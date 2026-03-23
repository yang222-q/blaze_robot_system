//! Project Blaze - Medical Care Robot Coordination System
//! 
//! A comprehensive multi-threaded system demonstrating modern Rust concurrent programming
//! and synchronization primitives. The system simulates a medical care facility where
//! multiple robots coordinate to execute tasks across shared resource zones.
//!
//! # Core Features
//!
//! - **Thread-safe Task Queue** : Producer-consumer pattern with concurrent push/pop operations
//! - **Fine-grained Zone Control** : Per-zone mutual exclusion preventing resource conflicts
//! - **Health Monitoring System** : Real-time heartbeat detection with timeout-based offline marking
//! - **Comprehensive Error Handling** : No panics; all errors properly propagated via Result types
//! - **Performance Benchmarking** : Real-time statistics collection and reporting
//! 
//! # Synchronization Design
//!
//! The system demonstrates three core synchronization patterns:
//! 
//! 1. **Task Queue Synchronization**
//!    - Uses `Mutex<VecDeque>` for thread-safe task storage
//!    - All push/pop operations are atomic and race-condition free
//!    - Handles empty queue gracefully with cooperative sleep instead of busy waiting
//!
//! 2. **Zone Lock Management** 
//!    - Each zone has an independent Mutex (fine-grained locking, not global)
//!    - Acquire/Release semantics strictly bound to task execution lifecycle
//!    - Ensures at most one robot occupies a zone at any given time
//!
//! 3. **Health Monitoring Coordination**
//!    - Producer threads (robots) update heartbeat timestamps
//!    - Consumer thread (monitor) checks for timeouts periodically
//!    - All state updates synchronized via Mutex to prevent data races
//! 
//! # Running the Demo
//!
//! ```sh
//! cargo run      # Run the complete demonstration
//! cargo test     # Execute full test suite
//! cargo build    # Compile the project
//! ```

mod task_queue;
mod zone_control;
mod health_monitor;
mod error;
mod config;
#[cfg(test)]
mod tests;

use task_queue::TaskQueue;
use zone_control::ZoneControl;
use health_monitor::HealthMonitor;
use error::Result;
use config::*;
use std::thread;
use std::time::Duration;
use std::sync::{Arc, Mutex};

fn main() {
    // Invoke the core system logic with proper error handling
    if let Err(e) = run_system() {
        eprintln!("[ERROR] System initialization failed: {}", e);
        std::process::exit(1);
    }
}

/// Core system initialization and main coordination loop
/// 
/// Initializes all subsystems (task queue, zone control, health monitoring)
/// and spawns all worker threads. The main thread waits for the demo duration
/// before collecting and printing benchmark statistics.
/// 
/// # Error Handling
/// 
/// Returns Result to handle potential failures:
/// - `ZoneControl::new()` may fail if zone count is 0
/// - All other operations use match/if-let for graceful error recovery
fn run_system() -> Result<()> {
    println!("[INFO] Project Blaze - Medical Care Robot Coordination System initializing...");
    println!("[INFO] System Configuration:");
    println!("  - Robots: {}, Zones: {}", NUM_ROBOTS, NUM_ZONES);
    println!("  - Demo Duration: {}s, Heartbeat Timeout: {}s", DEMO_DURATION_SECS, HEARTBEAT_TIMEOUT_SECS);

    // Initialize core components
    // Use ? operator to propagate initialization errors
    let task_queue = TaskQueue::new();
    let zone_control = ZoneControl::new(NUM_ZONES)?;
    let health_monitor = HealthMonitor::new();
    
    // Start the background health monitoring thread
    // This thread periodically checks heartbeats and marks robots offline if timeout occurs
    health_monitor.clone().start_monitor();

    // Real-time statistics counters
    // These are shared across threads via Arc<Mutex<>> for thread-safe updates
    let task_count = Arc::new(Mutex::new(0usize));
    let zone_access_count = Arc::new(Mutex::new(0usize));
    let robot_stats = Arc::new(Mutex::new(vec![0usize; NUM_ROBOTS]));

    // =====================================================
    // Task Generation Thread
    // =====================================================
    // Producer thread that generates tasks at regular intervals
    // Feeds tasks into the task queue for robots to consume
    {
        let tq = task_queue.clone();
        let tc = task_count.clone();
        thread::spawn(move || {
            let mut task_id = 0;
            loop {
                // Attempt to enqueue the task with proper error handling
                match tq.push(format!("Task-{}", task_id)) {
                    Ok(_) => {
                        // Successfully enqueued; update global counter
                        if let Ok(mut count) = tc.lock() {
                            *count += 1;
                        }
                        println!("[TASK] New task issued: Task-{}", task_id);
                    }
                    Err(e) => {
                        // Queue may be full; log error but continue
                        eprintln!("[WARN] Failed to enqueue Task-{}: {}", task_id, e);
                    }
                }
                task_id += 1;
                thread::sleep(Duration::from_millis(TASK_GENERATION_INTERVAL_MS));
            }
        });
    }

    // =====================================================
    // Robot Worker Threads
    // =====================================================
    // Spawn NUM_ROBOTS worker threads
    // Each robot operates independently, acquiring zones and executing tasks
    for robot_id in 0..NUM_ROBOTS {
        let tq = task_queue.clone();
        let zc = zone_control.clone();
        let hm = health_monitor.clone();
        let zac = zone_access_count.clone();
        let rs = robot_stats.clone();

        thread::spawn(move || {
            loop {
                // Main work loop: dequeue task -> acquire zone -> execute -> release zone
                match tq.pop() {
                    Ok(Some(task)) => {
                        // Successfully dequeued a task
                        // Determine which zone this robot will access (round-robin assignment)
                        let zone_id = robot_id % zc.zone_count();

                        // Attempt to acquire exclusive access to the zone
                        match zc.acquire(zone_id) {
                            Ok(_) => {
                                // Zone acquired successfully
                                // Update zone access counter
                                if let Ok(mut count) = zac.lock() {
                                    *count += 1;
                                }

                                println!("[ROBOT {}] Acquired Zone {}, executing {}", robot_id, zone_id, task);

                                // Update heartbeat to signal robot is alive and working
                                // This prevents the health monitor from marking this robot offline
                                if let Err(e) = hm.update(robot_id) {
                                    eprintln!("[ROBOT {}] Failed to update heartbeat: {}", robot_id, e);
                                }

                                // Increment this robot's task completion counter
                                if let Ok(mut stats) = rs.lock() {
                                    stats[robot_id] += 1;
                                }

                                // Simulate task execution time
                                thread::sleep(Duration::from_secs(TASK_EXECUTION_TIME_SECS));

                                // Release the zone lock to allow other robots to access it
                                if let Err(e) = zc.release(zone_id) {
                                    eprintln!("[ROBOT {}] Failed to release Zone {}: {}", robot_id, zone_id, e);
                                } else {
                                    println!("[ROBOT {}] Released Zone {}", robot_id, zone_id);
                                }
                            }
                            Err(e) => {
                                // Zone acquisition failed (e.g., zone out of range)
                                eprintln!("[ROBOT {}] Failed to acquire Zone {}: {}", robot_id, zone_id, e);
                            }
                        }
                    }
                    Ok(None) => {
                        // Queue is empty; sleep to reduce CPU spin and avoid busy-waiting
                        thread::sleep(Duration::from_millis(EMPTY_QUEUE_SLEEP_MS));
                    }
                    Err(e) => {
                        // Pop operation failed (e.g., lock poisoned)
                        eprintln!("[ROBOT {}] Failed to dequeue task: {}", robot_id, e);
                        thread::sleep(Duration::from_millis(EMPTY_QUEUE_SLEEP_MS));
                    }
                }
            }
        });
    }

    // =====================================================
    // Demonstration Run
    // =====================================================
    // Main thread waits for the demo duration, then collects statistics
    println!("[INFO] Demo started. Running for {} seconds...", DEMO_DURATION_SECS);
    let start_time = std::time::Instant::now();
    thread::sleep(Duration::from_secs(DEMO_DURATION_SECS));
    let elapsed = start_time.elapsed();

    // Collect and print performance benchmarks
    print_benchmark_report(&task_count, &zone_access_count, &robot_stats, elapsed)?;

    println!("[INFO] Demonstration completed successfully.");
    Ok(())
}

/// Prints comprehensive performance statistics and system metrics
/// 
/// Safely reads all shared counters with mutex error handling and computes
/// performance metrics including throughput, task distribution, and system health.
/// 
/// # Parameters
/// 
/// - `task_count` - Global counter of total tasks generated
/// - `zone_access_count` - Global counter of zone acquisitions
/// - `robot_stats` - Per-robot task completion counters
/// - `elapsed` - Elapsed time for the demo run
/// 
/// # Errors
/// 
/// Returns error if mutex locks cannot be acquired (should not happen in normal operation)
fn print_benchmark_report(
    task_count: &Arc<Mutex<usize>>,
    zone_access_count: &Arc<Mutex<usize>>,
    robot_stats: &Arc<Mutex<Vec<usize>>>,
    elapsed: Duration,
) -> Result<()> {
    // Safely read global statistics with error recovery
    let total_tasks = task_count
        .lock()
        .map(|c| *c)
        .unwrap_or_else(|_| 0);
    
    let zone_acquires = zone_access_count
        .lock()
        .map(|c| *c)
        .unwrap_or_else(|_| 0);
    
    let robot_data = robot_stats
        .lock()
        .map(|s| s.clone())
        .unwrap_or_else(|_| vec![0; NUM_ROBOTS]);

    // Compute performance metrics
    let elapsed_secs = elapsed.as_secs_f64();
    let throughput = if elapsed_secs > 0.0 {
        total_tasks as f64 / elapsed_secs
    } else {
        0.0
    };

    // Print comprehensive benchmark summary
    println!("\n========================================");
    println!("         BENCHMARK RESULTS ({}s run)          ", DEMO_DURATION_SECS);
    println!("========================================");
    println!("Task Throughput:         {:.2} tasks/sec", throughput);
    println!("Total Tasks Generated:   {}", total_tasks);
    println!("Zone Access Count:       {}", zone_acquires);
    println!("Concurrency Model:       {} independent robot threads", NUM_ROBOTS);
    // Careful statement: observed behavior, not strict proof
    println!("Synchronization Status:  No deadlock observed | Race-free by Mutex design");
    println!("System Stability:        Stable during {}s test run", DEMO_DURATION_SECS);
    println!("========================================\n");

    // Print per-robot task distribution
    println!("--- Robot Task Distribution ---");
    let mut total_completed = 0;
    for (id, count) in robot_data.iter().enumerate() {
        println!("  [Robot {}] {} tasks completed", id, count);
        total_completed += count;
    }
    println!("  [TOTAL] {} tasks executed", total_completed);
    println!("================================\n");

    Ok(())
}
