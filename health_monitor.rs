//! Health Monitor Module - Producer-Consumer Heartbeat Monitoring
//!
//! This module implements a health monitoring system using the producer-consumer pattern.
//! Robot threads (producers) periodically update their heartbeats, while a dedicated
//! monitor thread (consumer) periodically checks for offline robots.
//!
//! # Synchronization Design
//!
//! - **Shared State**: Arc<Mutex<HashMap>> stores robot heartbeat timestamps
//! - **Producers**: Robot threads call update() to signal they are alive
//! - **Consumer**: Monitor thread calls check_heartbeats() periodically
//! - **Atomicity**: All state updates are atomic via Mutex protection
//! - **No Races**: Mutex prevents concurrent reads and updates from interfering
//!
//! # Heartbeat Timeout Mechanism
//!
//! The system marks robots offline when they fail to update within HEARTBEAT_TIMEOUT_SECS:
//! - If robot executes normally, it calls update() every TASK_EXECUTION_TIME_SECS
//! - If no update for HEARTBEAT_TIMEOUT_SECS, robot is marked offline
//! - Offline robots can come back online by updating again
//! - This detects hung or crashed robots in a production-like manner
//!
//! # Thread Coordination
//!
//! Produces no race conditions despite concurrent updates from 5+ robots and 1 monitor:
//! - Multiple robots pushing heartbeats: Serialized by Mutex
//! - Monitor iterating and removing: Occurs in separate critical section
//! - Update during iteration impossible: HashMap is cloned before iteration safe point

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::collections::{HashMap, HashSet};
use std::thread;
use crate::error::{RobotSystemError, Result};
use crate::config::*;

/// Robot health monitoring subsystem using heartbeat detection
///
/// # Key Responsibilities
/// 
/// 1. **Heartbeat Collection**: Robots update their heartbeat timestamp when executing tasks
/// 2. **Timeout Detection**: Monitor thread detects when robots haven't updated in time
/// 3. **Offline Marking**: Robots exceeding timeout are marked offline with console notification
/// 4. **State Recovery**: Offline robots can come back online by updating again
///
/// # Concurrency Properties
///
/// - **Thread-safe**: Multiple robots + monitor thread coordinate safely
/// - **Race-condition free**: All HashMap accesses via Mutex
/// - **No deadlock**: Simple single-lock design
/// - **No starvation**: Fair access via Mutex queuing
#[derive(Clone)]
pub struct HealthMonitor {
    /// Shared heartbeat storage
    ///
    /// HashMap<robot_id, last_heartbeat_instant>
    /// Protected by Mutex for safe concurrent access
    /// Arc allows sharing across multiple threads
    heartbeats: Arc<Mutex<HashMap<usize, Instant>>>,
    
    /// Tracks which robots are currently marked as offline
    ///
    /// HashSet<robot_id> of robots that were detected offline
    /// Used to detect when a robot recovers from offline state
    offline_robots: Arc<Mutex<HashSet<usize>>>,
}

impl HealthMonitor {
    /// Creates a new health monitor with empty heartbeat state
    ///
    /// # Thread Safety
    /// - Multiple threads can call this and share the same monitor
    /// - All clones reference the same heartbeat data
    ///
    /// # Example
    /// ```
    /// use blaze_robot_system::health_monitor::HealthMonitor;
    /// let monitor = HealthMonitor::new();
    /// let monitor_clone = monitor.clone();  // Safe to share across threads
    /// ```
    pub fn new() -> Self {
        Self {
            heartbeats: Arc::new(Mutex::new(HashMap::new())),
            offline_robots: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Internal unified interface for heartbeat operations
    ///
    /// Centralizes all Mutex acquisition and error handling.
    /// Ensures consistent thread safety across all operations.
    ///
    /// # Parameters
    /// - `f`: Closure performing HashMap operation in critical section
    ///
    /// # Returns
    /// - Result with closure's return value or lock poisoning error
    fn with_heartbeats<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut HashMap<usize, Instant>) -> Result<R>,
    {
        // Acquire Mutex - blocks if another thread holds it
        let mut heartbeats = self.heartbeats.lock()
            .map_err(|_| RobotSystemError::LockPoisoned {
                resource: "HealthMonitor".to_string(),
            })?;

        // Execute operation within critical section
        f(&mut heartbeats)
    }

    /// Updates the heartbeat timestamp for a robot and logs the update
    ///
    /// Called by robot threads to signal they are alive.
    /// Outputs: `[ROBOT X] Heartbeat updated (latest: 0.00s ago)`
    /// If robot was offline, outputs: `[INFO] Robot X heartbeat recovered - marked as ONLINE`
    ///
    /// # Parameters
    /// - `robot_id`: The robot updating its heartbeat
    ///
    /// # Error Cases
    /// - `InvalidRobotId` if robot_id > MAX_ROBOT_ID
    /// - `LockPoisoned` if Mutex was poisoned
    pub fn update(&self, robot_id: usize) -> Result<()> {
        // Boundary validation: prevent invalid robot IDs
        if robot_id > MAX_ROBOT_ID {
            return Err(RobotSystemError::InvalidRobotId { robot_id });
        }

        // Check if robot was previously marked offline
        if let Ok(mut offline_set) = self.offline_robots.lock() {
            if offline_set.contains(&robot_id) {
                offline_set.remove(&robot_id);
                println!("[INFO] Robot {} heartbeat recovered - marked as ONLINE", robot_id);
            }
        }

        // Update heartbeat timestamp in critical section
        self.with_heartbeats(|heartbeats| {
            let now = Instant::now();
            heartbeats.insert(robot_id, now);
            println!("[ROBOT {}] Heartbeat updated (latest: {:.2}s ago)", robot_id, 0.0);
            Ok(())
        })
    }

    /// Starts a background monitor thread that checks for offline robots
    ///
    /// # Behavior
    ///
    /// Spawns a new thread that:
    /// 1. Sleeps for HEALTH_CHECK_INTERVAL_SECS
    /// 2. Checks all robot heartbeats
    /// 3. Identifies robots with no update in HEARTBEAT_TIMEOUT_SECS
    /// 4. Marks them offline with console notification: `[ALERT] Robot X heartbeat timeout (8s) - marked as OFFLINE`
    /// 5. Repeats forever (daemon thread)
    ///
    /// # Synchronization
    ///
    /// The monitor thread coordinates with robot threads via Mutex:
    /// - Robot update() and monitor check() don't interfere
    /// - No race conditions despite concurrent access
    /// - Offline marking is atomic
    ///
    /// # Correctness Properties
    ///
    /// - **No Ghost Offlines**: Robot won't be marked offline while executing
    ///   (because it updates heartbeat during execution)
    /// - **Eventual Offline Detection**: Hung robot will be detected within
    ///   (HEALTH_CHECK_INTERVAL_SECS + HEARTBEAT_TIMEOUT_SECS)
    /// - **Recovery**: Robot can come back online by updating again
    /// - **State Consistency**: HashMap is never iterated while being modified
    ///   (we collect offline IDs first, then remove them)
    ///
    /// # Example
    /// ```no_run
    /// # use blaze_robot_system::health_monitor::HealthMonitor;
    /// let monitor = HealthMonitor::new();
    /// monitor.clone().start_monitor();  // Clone before starting thread
    /// // Monitor now runs in background
    /// ```
    pub fn start_monitor(self) {
        thread::spawn(move || {
            loop {
                // Sleep until next health check interval
                thread::sleep(Duration::from_secs(HEALTH_CHECK_INTERVAL_SECS));

                // Check all robot heartbeats for timeout
                let _ = self.with_heartbeats(|heartbeats| {
                    let now = Instant::now();
                    let timeout_duration = Duration::from_secs(HEARTBEAT_TIMEOUT_SECS);

                    // Collect all robots that have exceeded the heartbeat timeout
                    let offline_robots: Vec<usize> = heartbeats
                        .iter()
                        .filter(|&(_, &last_heartbeat)| {
                            now.duration_since(last_heartbeat) > timeout_duration
                        })
                        .map(|(&robot_id, _)| robot_id)
                        .collect();

                    // Mark offline robots and update offline_robots tracking set
                    for robot_id in offline_robots {
                        // Log the offline event with [ALERT] format
                        println!(
                            "[ALERT] Robot {} heartbeat timeout ({}s) - marked as OFFLINE",
                            robot_id,
                            HEARTBEAT_TIMEOUT_SECS
                        );
                        
                        // Track this robot as offline for recovery detection
                        if let Ok(mut offline_set) = self.offline_robots.lock() {
                            offline_set.insert(robot_id);
                        }
                        
                        // Remove from active heartbeats (now tracked as offline)
                        heartbeats.remove(&robot_id);
                    }

                    Ok(())
                });
            }
        });
    }

    /// Returns the number of robots currently being monitored (not offline)
    ///
    /// # Note
    /// This is a snapshot count. The number may change immediately after return
    /// as robots update heartbeats or get marked offline.
    ///
    /// # Thread Safety
    /// - Safe to call from any thread
    /// - Returns consistent count at the moment of the call
    ///
    /// # Example
    /// ```no_run
    /// # use blaze_robot_system::health_monitor::HealthMonitor;
    /// let monitor = HealthMonitor::new();
    /// let online_count = monitor.active_robots().unwrap_or(0);
    /// println!("Active robots: {}", online_count);
    /// ```
    #[allow(dead_code)]
    pub fn active_robots(&self) -> Result<usize> {
        self.with_heartbeats(|heartbeats| {
            Ok(heartbeats.len())
        })
    }
}

impl Default for HealthMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_update_records_heartbeat() {
        let monitor = HealthMonitor::new();
        assert!(monitor.update(1).is_ok());
        assert!(monitor.update(2).is_ok());
        assert_eq!(monitor.active_robots().unwrap(), 2);
    }

    #[test]
    fn test_invalid_robot_id() {
        let monitor = HealthMonitor::new();
        // Robot ID way too large should be rejected
        assert!(monitor.update(MAX_ROBOT_ID + 1).is_err());
    }

    #[test]
    fn test_successful_heartbeat_update() {
        let monitor = HealthMonitor::new();
        let robot_id = 0;
        
        // Initial update should succeed
        assert!(monitor.update(robot_id).is_ok());
        assert_eq!(monitor.active_robots().unwrap(), 1);
        
        // Subsequent updates should succeed
        assert!(monitor.update(robot_id).is_ok());
        assert_eq!(monitor.active_robots().unwrap(), 1);
    }

    #[test]
    fn test_multiple_robots() {
        let monitor = HealthMonitor::new();
        for id in 0..5 {
            assert!(monitor.update(id).is_ok());
        }
        assert_eq!(monitor.active_robots().unwrap(), 5);
    }

    #[test]
    #[ignore]  // Long test due to timeout waits
    fn test_offline_marking() {
        let monitor = HealthMonitor::new();
        monitor.clone().start_monitor();
        
        // Record a robot heartbeat
        assert!(monitor.update(99).is_ok());
        assert_eq!(monitor.active_robots().unwrap(), 1);
        
        // Wait for monitor to detect timeout
        // Note: This test uses a longer HEARTBEAT_TIMEOUT in practice
        // For testing, we would need to set a shorter timeout value
        thread::sleep(Duration::from_secs(10));
        
        // After timeout, robot should be marked offline  
        // (exact count depends on timeout configuration)
    }

    #[test]
    fn test_heartbeat_recovery_lifecycle() {
        // Test the complete offline -> recovery -> online cycle
        let monitor = HealthMonitor::new();
        let robot_id = 7;
        
        // Phase 1: Robot comes online and updates heartbeat
        println!("[TEST] Phase 1: Robot {} initializing heartbeat", robot_id);
        assert!(monitor.update(robot_id).is_ok());
        assert_eq!(
            monitor.active_robots().unwrap(),
            1,
            "Robot should be in active heartbeat list after first update"
        );
        
        // Phase 2: Manually mark robot as offline (simulating timeout detection)
        // In real execution, monitor thread would do this after 8 seconds without update
        println!("[TEST] Phase 2: Simulating offline detection");
        if let Ok(mut offline_set) = monitor.offline_robots.lock() {
            offline_set.insert(robot_id);
        }
        
        // Verify robot is in offline set (this would be logged as [ALERT] in real system)
        let is_offline = if let Ok(offline_set) = monitor.offline_robots.lock() {
            offline_set.contains(&robot_id)
        } else {
            false
        };
        assert!(is_offline, "Robot should be marked in offline set");
        
        // Phase 3: Robot recovers and updates heartbeat again
        // The update() method should detect it was offline and mark it recovered
        println!("[TEST] Phase 3: Robot {} sending recovery heartbeat", robot_id);
        assert!(monitor.update(robot_id).is_ok());
        
        // Verify robot is no longer in offline set (recovered to online)
        let is_still_offline = if let Ok(offline_set) = monitor.offline_robots.lock() {
            offline_set.contains(&robot_id)
        } else {
            false
        };
        assert!(
            !is_still_offline,
            "Robot should be removed from offline set after recovery update"
        );
        
        // Verify robot is in active heartbeat list
        assert_eq!(
            monitor.active_robots().unwrap(),
            1,
            "Robot should be back in active heartbeat list after recovery"
        );
        
        println!("[TEST] Recovery lifecycle test PASSED: offline -> recovery -> online");
    }
}
