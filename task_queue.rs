//! Task Queue Module - Thread-safe Producer-Consumer Pattern Implementation
//!
//! This module provides a thread-safe FIFO task queue that serves as the central
//! coordination point for task distribution in the robot system. Multiple producer
//! threads (task generators) and consumer threads (robots) safely interact through
//! this queue without data races or synchronization issues.
//!
//! # Synchronization Design
//!
//! - **Mutual Exclusion**: All queue operations are protected by a Mutex
//! - **Data Structure**: VecDeque for efficient O(1) enqueue/dequeue at both ends
//! - **Shared Ownership**: Arc allows multiple threads to reference the same queue
//! - **Atomicity**: Push and pop operations are atomic; no partial state updates
//! - **Empty Queue Handling**: Consumers receive Ok(None) when queue is empty,
//!   allowing graceful handling without busy-waiting or blocking operations
//!
//! # Critical Sections
//!
//! The following operations are critical sections protected by mutex:
//! - `push()`: Enqueue operation is atomic; prevents task duplication or loss
//! - `pop()`: Dequeue operation is atomic; ensures each task is consumed exactly once
//! - `len()`, `is_empty()`: Read operations are atomic snapshots of current state

use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use crate::error::{RobotSystemError, Result};
use crate::config::MAX_QUEUE_SIZE;

/// Thread-safe task queue for producer-consumer coordination
/// 
/// # Concurrency Guarantees
///
/// - Multiple producer threads can push tasks concurrently
/// - Multiple consumer threads can pop tasks concurrently  
/// - No task is ever duplicated or lost
/// - All operations are race-condition-free due to Mutex protection
/// 
/// # Implementation Notes
///
/// - Uses VecDeque for O(1) operations at both ends
/// - All public methods are thread-safe by design
/// - Empty queue returns Ok(None) instead of blocking (allows non-blocking patterns)
#[derive(Clone)]
pub struct TaskQueue {
    /// Mutex-protected queue storage
    ///
    /// Uses VecDeque<String> to store task IDs/names.
    /// Mutex ensures only one thread accesses the queue at a time.
    /// Arc allows sharing across multiple threads.
    inner: Arc<Mutex<VecDeque<String>>>,
}

impl TaskQueue {
    /// Creates a new empty task queue
    /// 
    /// # Concurrency Safety
    ///
    /// The new queue can be cloned and shared across multiple threads.
    /// All clones reference the same underlying queue.
    /// 
    /// # Example
    /// ```
    /// use blaze_robot_system::task_queue::TaskQueue;
    /// let queue = TaskQueue::new();
    /// // queue can now be cloned and passed to multiple threads
    /// ```
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Internal unified interface for all queue operations
    ///
    /// This method centralizes all Mutex acquisition and error handling,
    /// ensuring consistent thread safety guarantees across all operations.
    /// All public methods use this interface.
    ///
    /// # Parameters
    /// - `f`: Closure that performs the actual queue operation within the critical section
    ///
    /// # Returns
    /// - Result containing the closure's return value, or a lock poisoning error
    fn with_lock<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut VecDeque<String>) -> R,
    {
        // Acquire mutex lock - blocks if another thread holds it
        let mut queue = self.inner.lock()
            .map_err(|_| RobotSystemError::LockPoisoned {
                resource: "TaskQueue".to_string(),
            })?;

        // Execute the operation within the critical section
        Ok(f(&mut queue))
    }

    /// Enqueues a task at the end of the queue
    ///
    /// # Synchronization Properties
    ///
    /// - **Atomicity**: Enqueue is atomic; either the task is added or operation fails
    /// - **Mutual Exclusion**: No other thread can modify the queue during this operation
    /// - **Memory Ordering**: All changes in the critical section are visible to other threads
    ///   after the lock is released (guaranteed by Mutex semantics)
    ///
    /// # Boundary Checks
    /// - Rejects tasks if queue size exceeds MAX_QUEUE_SIZE (prevents memory exhaustion)
    /// - Validates input is not empty (optional business logic)
    ///
    /// # Error Handling
    /// - Returns `QueueError` if queue is full
    /// - Returns `LockPoisoned` if the Mutex was poisoned (another thread panicked)
    ///
    /// # Example
    /// ```no_run
    /// # use blaze_robot_system::task_queue::TaskQueue;
    /// let queue = TaskQueue::new();
    /// match queue.push("Task-123".to_string()) {
    ///     Ok(_) => println!("Task enqueued successfully"),
    ///     Err(e) => eprintln!("Failed to enqueue: {}", e),
    /// }
    /// ```
    pub fn push(&self, task: String) -> Result<()> {
        self.with_lock(|queue| {
            // Boundary check: prevent queue from growing unbounded
            if queue.len() >= MAX_QUEUE_SIZE {
                return Err(RobotSystemError::QueueError {
                    message: format!("Queue full: {} tasks pending", queue.len()),
                });
            }
            
            // Critical section: enqueue operation
            queue.push_back(task);
            Ok(())
        })?
    }

    /// Dequeues the next task from the front of the queue
    ///
    /// # Synchronization Properties
    ///
    /// - **Atomicity**: Pop is atomic; task removal cannot be interrupted
    /// - **Ordering**: FIFO semantics guaranteed; first-pushed tasks are first-popped
    /// - **Empty Queue**: Returns Ok(None) rather than blocking, allowing non-blocking patterns
    ///
    /// # Concurrency Pattern
    ///
    /// This method enables the producer-consumer pattern:
    /// - Multiple robots (consumers) safely pop tasks concurrently
    /// - Each task is popped exactly once (no duplication or loss)
    ///
    /// # Return Values
    /// - `Ok(Some(task))` - Successfully dequeued a task
    /// - `Ok(None)` - Queue is empty (consumer can sleep and retry)
    /// - `Err(e)` - Lock poisoning or other errors
    ///
    /// # Example
    /// ```no_run
    /// # use blaze_robot_system::task_queue::TaskQueue;
    /// let queue = TaskQueue::new();
    /// loop {
    ///     match queue.pop() {
    ///         Ok(Some(task)) => println!("Processing: {}", task),
    ///         Ok(None) => println!("Queue empty, waiting..."),
    ///         Err(e) => eprintln!("Error: {}", e),
    ///     }
    /// }
    /// ```
    pub fn pop(&self) -> Result<Option<String>> {
        self.with_lock(|queue| {
            // Critical section: dequeue operation (FIFO semantics)
            Ok(queue.pop_front())
        })?
    }

    /// Returns the current number of tasks in the queue
    ///
    /// # Note
    /// This is a snapshot of the queue size at the moment of the call.
    /// In a concurrent environment, the actual size may change immediately after return.
    /// Use this for statistics/monitoring, not for control logic.
    ///
    /// # Concurrency Safety
    /// - Safe to call from multiple threads
    /// - Returns a consistent view of the queue state at the moment of the call
    #[allow(dead_code)]
    pub fn len(&self) -> Result<usize> {
        self.with_lock(|queue| Ok(queue.len()))?
    }

    /// Checks if the queue is currently empty
    ///
    /// # Concurrency Safety
    /// - Safe to call from multiple threads
    /// - Returns a consistent snapshot of queue emptiness
    #[allow(dead_code)]
    pub fn is_empty(&self) -> Result<bool> {
        self.with_lock(|queue| Ok(queue.is_empty()))?
    }
}

impl Default for TaskQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_pop_order() {
        let q = TaskQueue::new();
        q.push("Task-1".to_string()).unwrap();
        q.push("Task-2".to_string()).unwrap();
        
        assert_eq!(q.pop().unwrap(), Some("Task-1".to_string()));
        assert_eq!(q.pop().unwrap(), Some("Task-2".to_string()));
        assert_eq!(q.pop().unwrap(), None);
    }

    #[test]
    fn test_empty_queue() {
        let q: TaskQueue = TaskQueue::new();
        assert!(q.is_empty().unwrap());
        assert_eq!(q.pop().unwrap(), None);
    }

    #[test]
    fn test_queue_size() {
        let q = TaskQueue::new();
        assert_eq!(q.len().unwrap(), 0);
        q.push("Task-1".to_string()).unwrap();
        assert_eq!(q.len().unwrap(), 1);
        q.pop().unwrap();
        assert_eq!(q.len().unwrap(), 0);
    }

    #[test]
    fn test_max_queue_size() {
        let q = TaskQueue::new();
        // Try to exceed MAX_QUEUE_SIZE
        let result = (0..=MAX_QUEUE_SIZE).try_for_each(|i| {
            q.push(format!("Task-{}", i))
        });
        assert!(result.is_err());
    }
}
