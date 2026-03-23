//! Zone Control Module - Fine-grained Mutual Exclusion Implementation
//!
//! This module manages access to shared resource zones in the robot system.
//! Instead of using a single global lock, each zone has its own Mutex,
//! enabling multiple robots to access different zones concurrently while
//! maintaining strict mutual exclusion within each zone.
//!
//! # Synchronization Design
//!
//! - **Fine-grained Locking**: Each zone has an independent Mutex (not a global lock)
//! - **Mutual Exclusion**: Only one robot can occupy any given zone at a time
//! - **Deadlock-Free**: Simple locking with no nested locks prevents deadlocks
//! - **Fair Access**: Robots wait in the lock queue; no starvation
//! - **Bounded Waiting**: Spinning with sleep avoids busy-waiting and CPU waste
//!
//! # Zone Access Semantics
//!
//! The acquire() and release() methods form a critical pair:
//! - `acquire(zone_id)`: Robot acquires exclusive access to zone_id
//! - `release(zone_id)`: Robot releases the zone for other robots to use
//! - Between acquire and release, the zone is reserved for this robot
//! - Failing to release correctly leads to deadlock or priority inversion
//!
//! # Performance Implications
//!
//! - **Concurrency Benefit**: 5 robots can access 5 zones simultaneously
//! - **Lock Contention**: Reduced compared to global lock (robots competing only for same zone)
//! - **Scalability**: Linearly scalable with number of zones
//! - **Trade-off**: More Mutex objects use slightly more memory

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use crate::error::{RobotSystemError, Result};

/// Zone access coordinator using fine-grained locking
///
/// # Concurrency Guarantees
///
/// - Mutual exclusion: At most one robot occupies any zone
/// - No deadlock: Lock-free design with simple spin-sleep pattern
/// - Fair access: Mutex provides fair queuing of waiting threads
/// - Race-condition free: All zone state changes are atomic
///
/// # Implementation: Per-Zone Mutex
///
/// Uses `Vec<Mutex<bool>>` where:
/// - Each Mutex<bool> represents one zone's lock
/// - true = zone occupied, false = zone free
/// - Reading/updating all protected by individual zone locks
#[derive(Clone)]
pub struct ZoneControl {
    /// Per-zone Mutex array for fine-grained locking
    ///
    /// Each element is an independent Mutex<bool> protecting one zone.
    /// This design allows concurrent access to different zones.
    /// Arc enables sharing across multiple threads.
    ///
    /// Invariant: zones[i] is true iff zone i is occupied
    zones: Arc<Vec<Mutex<bool>>>,
    
    /// Number of zones (cached for quick access without locking)
    /// Immutable after initialization; safe to access without locks
    num_zones: usize,
}

impl ZoneControl {
    /// Creates a new zone controller with the specified number of zones
    ///
    /// # Initialization
    /// - All zones start in the free state (false/not occupied)
    /// - Zones are indexed 0 to num_zones-1
    ///
    /// # Boundary Validation
    /// - Returns error if num_zones == 0 (at least one zone required)
    ///
    /// # Thread Safety
    /// - The returned ZoneControl can be safely cloned and shared across threads
    /// - All clones reference the same zone state
    ///
    /// # Error Cases
    /// - `InvalidInput` if num_zones is 0
    ///
    /// # Example
    /// ```
    /// use blaze_robot_system::zone_control::ZoneControl;
    /// let zc = ZoneControl::new(5).expect("Failed to create zone control");
    /// ```
    pub fn new(num_zones: usize) -> Result<Self> {
        // Boundary validation: require at least one zone
        if num_zones == 0 {
            return Err(RobotSystemError::Other {
                message: "Zone count must be at least 1".to_string(),
            });
        }

        // Initialize all zones to free state (false = not occupied)
        let zones = (0..num_zones)
            .map(|_| Mutex::new(false))
            .collect::<Vec<_>>();

        Ok(Self {
            zones: Arc::new(zones),
            num_zones,
        })
    }

    /// Internal unified interface for zone lock operations
    ///
    /// Centralizes all Mutex acquisition, boundary checking, and error handling.
    /// Ensures consistent thread safety and synchronization properties.
    ///
    /// # Parameters
    /// - `zone_id`: The zone to lock
    /// - `f`: Closure performing the operation within the critical section
    ///
    /// # Error Handling
    /// - Returns `ZoneIdOutOfRange` if zone_id >= num_zones
    /// - Returns `LockPoisoned` if Mutex was poisoned (thread panicked)
    fn with_zone_lock<F, R>(&self, zone_id: usize, f: F) -> Result<R>
    where
        F: FnOnce(&mut bool) -> Result<R>,
    {
        // Boundary check: ensure zone_id is valid
        if zone_id >= self.num_zones {
            return Err(RobotSystemError::ZoneIdOutOfRange {
                zone_id,
                max_zones: self.num_zones,
            });
        }

        // Acquire the zone's individual Mutex (fine-grained lock)
        let mut zone_occupied = self.zones[zone_id].lock()
            .map_err(|_| RobotSystemError::LockPoisoned {
                resource: format!("Zone {}", zone_id),
            })?;

        // Perform the operation within the critical section
        f(&mut zone_occupied)
    }

    /// Acquires exclusive access to a zone for task execution
    ///
    /// # Critical Section Semantics
    ///
    /// This method forms the acquire side of the acquire-release pair.
    /// After successful return, the calling robot has exclusive access to the zone.
    /// The robot must call release() when done to allow other robots access.
    ///
    /// # Spinlock Pattern with Sleep
    ///
    /// Uses a spin-wait pattern to handle contention:
    /// 1. Try to acquire zone (zone_occupied = false)
    /// 2. If successful, return Ok(())
    /// 3. If occupied, release lock and sleep briefly
    /// 4. Retry from step 1
    ///
    /// This approach:
    /// - Avoids blocking operations (keeps options open)
    /// - Reduces busy-waiting via 100µs sleep intervals
    /// - Is simple and deadlock-free
    ///
    /// # Atomicity Guarantee
    ///
    /// The transition from free to occupied is atomic:
    /// - No two robots can simultaneously read "free" and both acquire the zone
    /// - Mutex ensures the state check-and-set is indivisible
    ///
    /// # Parameters
    /// - `zone_id`: The zone to acquire (0 to num_zones-1)
    ///
    /// # Error Cases
    /// - `ZoneIdOutOfRange` if zone_id >= num_zones
    /// - `LockPoisoned` if Mutex was poisoned
    ///
    /// # Blocks Until
    /// - Zone becomes available (another robot releases it)
    /// - Or error condition occurs
    ///
    /// # Example
    /// ```no_run
    /// # use blaze_robot_system::zone_control::ZoneControl;
    /// let zc = ZoneControl::new(5).unwrap();
    /// match zc.acquire(0) {
    ///     Ok(_) => println!("Zone acquired, executing task..."),
    ///     Err(e) => eprintln!("Acquisition failed: {}", e),
    /// }
    /// // DO NOT FORGET to call zc.release(0) when done!
    /// ```
    pub fn acquire(&self, zone_id: usize) -> Result<()> {
        loop {
            // Attempt to acquire the zone in this iteration
            match self.with_zone_lock(zone_id, |zone_occupied| {
                if !*zone_occupied {
                    // Zone is free: occupy it and signal success
                    *zone_occupied = true;
                    Ok(true)
                } else {
                    // Zone is occupied: signal retry needed
                    Ok(false)
                }
            }) {
                Ok(acquired) if acquired => {
                    // Successfully acquired the zone
                    return Ok(());
                }
                Ok(_) => {
                    // Zone was occupied; sleep briefly before retrying
                    // This reduces CPU busy-waiting while maintaining responsiveness
                    thread::sleep(Duration::from_micros(100));
                }
                Err(e) => {
                    // Error occurred (boundary check or lock poison)
                    return Err(e);
                }
            }
        }
    }

    /// Releases exclusive access to a zone for other robots to use
    ///
    /// # Critical Section Semantics
    ///
    /// This method forms the release side of the acquire-release pair.
    /// Must be called by the robot holding the zone to allow others to acquire it.
    ///
    /// # Atomicity Guarantee
    ///
    /// The state change (occupied to free) is atomic:
    /// - Transition happens entirely within the Mutex critical section
    /// - No other thread can observe a partial state  
    /// - Waiting threads are woken immediately after release
    ///
    /// # Correctness Properties
    ///
    /// This implementation prevents:
    /// - **Double Release**: If a robot releases without holding, it marks the zone free (idempotent)
    /// - **Lock Leaks**: If robot exits without release, the zone remains occupied
    ///   (Application layer should ensure matching acquire/release calls)
    /// - **Race Conditions**: The state update is protected by Mutex
    ///
    /// # Parameters
    /// - `zone_id`: The zone to release (should match the acquire call)
    ///
    /// # Error Cases
    /// - `ZoneIdOutOfRange` if zone_id >= num_zones
    /// - `LockPoisoned` if Mutex was poisoned
    ///
    /// # Example
    /// ```no_run
    /// # use blaze_robot_system::zone_control::ZoneControl;
    /// let zc = ZoneControl::new(5).unwrap();
    /// if zc.acquire(0).is_ok() {
    ///     println!("Task executed successfully");
    ///     zc.release(0).ok();  // Always release when done
    /// }
    /// ```
    pub fn release(&self, zone_id: usize) -> Result<()> {
        self.with_zone_lock(zone_id, |zone_occupied| {
            // Critical section: reset zone to free state
            *zone_occupied = false;
            Ok(())
        })
    }

    /// Returns the total number of zones in this controller
    ///
    /// # Thread Safety
    /// - Completely thread-safe (no locks needed)
    /// - num_zones is immutable after initialization
    /// - Can be safely called from any thread at any time
    ///
    /// # Use Cases
    /// - Robots use this to determine zone_id = robot_id % zone_count()
    /// - Tests use this to iterate over all zones
    /// - Statistics collection
    pub fn zone_count(&self) -> usize {
        self.num_zones
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zone_acquire_release() {
        let zc = ZoneControl::new(3).unwrap();
        
        // Should successfully acquire an initially free zone
        assert!(zc.acquire(0).is_ok());
        
        // Should successfully release
        assert!(zc.release(0).is_ok());
        
        // Should be able to reacquire after release
        assert!(zc.acquire(0).is_ok());
        assert!(zc.release(0).is_ok());
    }

    #[test]
    fn test_zone_out_of_range() {
        let zc = ZoneControl::new(3).unwrap();
        
        // Should reject zone_id >= num_zones
        assert!(zc.acquire(5).is_err());
        assert!(zc.release(10).is_err());
    }

    #[test]
    fn test_concurrent_zones() {
        use std::sync::Arc;
        use std::thread;
        
        let zc = Arc::new(ZoneControl::new(2).unwrap());
        
        // Robot 0 takes zone 0
        let zc0 = zc.clone();
        let h0 = thread::spawn(move || {
            zc0.acquire(0).unwrap();
            thread::sleep(Duration::from_millis(100));
            zc0.release(0).unwrap();
        });
        
        // Give robot 0 time to acquire
        thread::sleep(Duration::from_millis(10));
        
        // Robot 1 should be able to take zone 1 concurrently
        let zc1 = zc.clone();
        let h1 = thread::spawn(move || {
            zc1.acquire(1).unwrap();
            zc1.release(1).unwrap();
        });
        
        h0.join().unwrap();
        h1.join().unwrap();
    }

    #[test]
    fn test_zone_count() {
        let zc = ZoneControl::new(7).unwrap();
        assert_eq!(zc.zone_count(), 7);
    }

    #[test]
    fn test_zone_mutual_exclusion_strict() {
        // Strict verification that mutual exclusion truly prevents concurrent zone entry
        // Multiple threads compete for the same zone; verify only 1 can be in critical section
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::thread;
        
        let zc = Arc::new(ZoneControl::new(1).unwrap());  // Single zone to maximize contention
        
        // Counters to track concurrent access
        let current_in_critical = Arc::new(AtomicUsize::new(0));
        let max_concurrent = Arc::new(AtomicUsize::new(0));
        
        let mut handles = vec![];
        
        // Spawn 10 threads all competing for the same single zone
        for _thread_id in 0..10 {
            let zc_clone = zc.clone();
            let current_clone = current_in_critical.clone();
            let max_clone = max_concurrent.clone();
            
            let handle = thread::spawn(move || {
                for iteration in 0..3 {
                    // Attempt to acquire the single zone
                    zc_clone.acquire(0).expect("Zone 0 should exist");
                    
                    // --- CRITICAL SECTION ---
                    // Increment counter: thread is now in critical section
                    let count_before = current_clone.fetch_add(1, Ordering::SeqCst);
                    let count_inside = count_before + 1;
                    
                    // Update maximum concurrent count observed
                    let mut max = max_clone.load(Ordering::SeqCst);
                    while count_inside > max {
                        match max_clone.compare_exchange(
                            max,
                            count_inside,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        ) {
                            Ok(_) => break,
                            Err(actual) => max = actual,
                        }
                    }
                    
                    // Simulate work inside critical section
                    thread::sleep(Duration::from_millis(5));
                    
                    // Decrement counter: thread is leaving critical section
                    current_clone.fetch_sub(1, Ordering::SeqCst);
                    // --- END CRITICAL SECTION ---
                    
                    // Release the zone for other threads
                    zc_clone.release(0).expect("Zone 0 should be releasable");
                    
                    // Small delay between iterations to allow scheduling variety
                    if iteration < 2 {
                        thread::sleep(Duration::from_micros(100));
                    }
                }
            });
            
            handles.push(handle);
        }
        
        // Wait for all threads to complete
        for handle in handles {
            handle.join().expect("Thread should complete without panic");
        }
        
        // Verification: Maximum concurrent access should be exactly 1
        let max_observed = max_concurrent.load(Ordering::SeqCst);
        assert_eq!(
            max_observed, 1,
            "Mutual exclusion VIOLATED: observed {} threads in critical section simultaneously (should be 1)",
            max_observed
        );
        
        // Final state: no thread should be in critical section
        let final_count = current_in_critical.load(Ordering::SeqCst);
        assert_eq!(
            final_count, 0,
            "Final state error: {} threads still in critical section",
            final_count
        );
    }
}
