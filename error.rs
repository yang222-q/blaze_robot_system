//! Error Handling Module
//! 
//! This module defines the system's error types and Result type alias.
//! All errors follow Rust's idiomatic error handling patterns (std::error::Error trait).
//! No panics should occur in business logic; all errors are properly propagated.
//!
//! # Error Types
//!
//! Each variant represents a specific failure mode that can occur in the system:
//! - `LockPoisoned`: Multithreading failure - Mutex was poisoned (thread panicked)
//! - `ZoneIdOutOfRange`: Invalid zone access - requested zone doesn't exist
//! - `InvalidRobotId`: Invalid robot ID - robot_id exceeds boundary
//! - `QueueError`: Queue operation failure - may be full or other issues
//! - `Other`: Generic error for unclassified failures

use std::fmt;
use std::error::Error;

/// Application-wide error type
/// 
/// This enum encompasses all possible failures in the robot coordination system.
/// Follows Rust best practices for error handling via std::error::Error trait.
/// Enables callers to handle specific error cases appropriately.
#[derive(Debug, Clone)]
pub enum RobotSystemError {
    /// Mutex lock was poisoned
    /// 
    /// Occurs when a thread holding a Mutex panics without releasing it.
    /// Indicates a serious failure in another thread, not recoverable.
    /// 
    /// # Context
    /// The `resource` field contains the name of the protected resource (e.g., "TaskQueue", "Zone 0")
    LockPoisoned {
        resource: String,
    },
    
    /// Zone ID is outside valid range
    ///
    /// Attempted to access a zone that doesn't exist.
    /// Valid zone IDs are 0 to (max_zones - 1).
    /// 
    /// # Context  
    /// - `zone_id`: The invalid ID that was requested
    /// - `max_zones`: The number of zones that actually exist
    ZoneIdOutOfRange {
        zone_id: usize,
        max_zones: usize,
    },
    
    /// Robot ID is invalid
    ///
    /// Attempted to use a robot ID outside the valid range (> MAX_ROBOT_ID).
    /// Helps prevent integer overflow issues in monitoring arrays.
    ///
    /// # Context
    /// - `robot_id`: The invalid robot ID that was used
    InvalidRobotId {
        robot_id: usize,
    },
    
    /// Task queue operation failed
    ///
    /// May indicate queue is full, internal consistency issues, or other problems.
    /// 
    /// # Context
    /// - `message`: Details about why the operation failed
    QueueError {
        message: String,
    },
    
    /// Generic error for other failures
    ///
    /// Catch-all for error conditions not covered by specific types.
    /// Should be converted to specific types when possible for better handling.
    ///
    /// # Context
    /// - `message`: Description of the error condition
    Other {
        message: String,
    },
}

impl fmt::Display for RobotSystemError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RobotSystemError::LockPoisoned { resource } => {
                write!(f, "[LOCK_POISON] Mutex poisoned on resource: {}", resource)
            }
            RobotSystemError::ZoneIdOutOfRange { zone_id, max_zones } => {
                write!(f, "[ZONE_OOB] Zone ID {} is out of range (valid: 0-{})", zone_id, max_zones - 1)
            }
            RobotSystemError::InvalidRobotId { robot_id } => {
                write!(f, "[INVALID_ROBOT] Robot ID {} exceeds maximum allowed", robot_id)
            }
            RobotSystemError::QueueError { message } => {
                write!(f, "[QUEUE_ERR] {}", message)
            }
            RobotSystemError::Other { message } => {
                write!(f, "[ERROR] {}", message)
            }
        }
    }
}

impl Error for RobotSystemError {}

/// Application Result type alias
/// 
/// Simplifies function signatures; all functions returning Result use this type
pub type Result<T> = std::result::Result<T, RobotSystemError>;
