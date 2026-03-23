//! Configuration module - Defines all tunable parameters for the system
//! 
//! This module centralizes all configurable parameters to allow easy system tuning
//! and testing without modifying core logic. Changing these values should be reflected
//! in system behavior and test scenarios.

/// Total number of robot worker threads in the system
pub const NUM_ROBOTS: usize = 5;

/// Total number of shared resource zones that robots must coordinate access to
pub const NUM_ZONES: usize = 5;

/// Total duration of the demonstration run in seconds
pub const DEMO_DURATION_SECS: u64 = 30;

/// Time interval for generating new tasks (in milliseconds)
pub const TASK_GENERATION_INTERVAL_MS: u64 = 800;

/// Duration of task execution simulation (in seconds)
pub const TASK_EXECUTION_TIME_SECS: u64 = 2;

/// Interval between health monitor checks (in seconds)
pub const HEALTH_CHECK_INTERVAL_SECS: u64 = 1;

/// Timeout threshold for marking a robot as offline (in seconds)
/// If no heartbeat is received within this duration, robot is marked offline
pub const HEARTBEAT_TIMEOUT_SECS: u64 = 8;

/// Sleep duration when queue is empty (in milliseconds) to reduce CPU spin
pub const EMPTY_QUEUE_SLEEP_MS: u64 = 100;

/// Maximum allowed tasks in queue before rejecting new tasks
pub const MAX_QUEUE_SIZE: usize = 10000;

/// Maximum valid robot ID
pub const MAX_ROBOT_ID: usize = 1000;
