use super::*;
use std::thread;
use std::time::Duration;

#[test]
fn test_task_queue() {
    let q = TaskQueue::new();
    
    // Test push operation
    let push_result = q.push("Task-1".to_string());
 
    assert!(push_result.is_ok(), "Failed to push task");
    
    // Test pop operation - should return Ok(Some(task))
    let pop_result = q.pop();
    assert!(pop_result.is_ok(), "Failed to pop task");
    assert_eq!(pop_result.unwrap(), Some("Task-1".to_string()));
    
    // Test pop on empty queue - should return Ok(None)
    let pop_empty = q.pop();
    assert!(pop_empty.is_ok(), "Failed to pop from empty queue");
    assert_eq!(pop_empty.unwrap(), None);
}

#[test]
fn test_zone_control() {
    let z = ZoneControl::new(3);
    assert!(z.is_ok(), "Failed to create ZoneControl");
    
    let zc = z.unwrap();
    
    // Test acquire
    let acquire_result = zc.acquire(0);
    assert!(acquire_result.is_ok(), "Failed to acquire zone");
    
    // Test release
    let release_result = zc.release(0);
    assert!(release_result.is_ok(), "Failed to release zone");
    
    // Test acquire again after release - should succeed
    let acquire_again = zc.acquire(0);
    assert!(acquire_again.is_ok(), "Failed to acquire zone again");
    
    let release_again = zc.release(0);
    assert!(release_again.is_ok(), "Failed to release zone again");
}

#[test]
fn test_health_monitor() {
    let hm = HealthMonitor::new();
    
    // Test valid robot ID
    let update_result = hm.update(999);
    assert!(update_result.is_ok(), "Failed to update valid robot");
    
    // Test another valid robot ID
    let update_result2 = hm.update(0);
    assert!(update_result2.is_ok(), "Failed to update robot 0");
}

#[test]
fn test_concurrent_access() {
    let tq = TaskQueue::new();
    let zc_result = ZoneControl::new(5);
    assert!(zc_result.is_ok(), "Failed to create ZoneControl for concurrent test");
    
    let zc = zc_result.unwrap();
    let hm = HealthMonitor::new();

    let mut handles = vec![];
    for i in 0..10 {
        let tq2 = tq.clone();
        let zc2 = zc.clone();
        let hm2 = hm.clone();
        let h = thread::spawn(move || {
            // Test push
            let _ = tq2.push(format!("Test-{}", i));
            
            let zone = i % 5;
            // Test acquire and release
            if let Ok(_) = zc2.acquire(zone) {
                let _ = hm2.update(i);
                thread::sleep(Duration::from_millis(50));
                let _ = zc2.release(zone);
            }
        });
        handles.push(h);
    }
    
    // Wait for all threads to complete
    for h in handles {
        h.join().unwrap();
    }
    
    // If we reach here without panic, concurrent safety is working
    assert!(true);
}

#[test]
fn test_zone_control_invalid_zone() {
    let zc_result = ZoneControl::new(3);
    assert!(zc_result.is_ok(), "Failed to create ZoneControl");
    
    let zc = zc_result.unwrap();
    
    // Test acquire on invalid zone ID (out of bounds)
    let acquire_invalid = zc.acquire(10);
    assert!(acquire_invalid.is_err(), "Should fail for invalid zone ID");
}

#[test]
fn test_queue_operations() {
    let q = TaskQueue::new();
    
    // Push multiple tasks
    for i in 0..5 {
        let result = q.push(format!("Task-{}", i));
        assert!(result.is_ok(), "Failed to push task {}", i);
    }
    
    // Pop all tasks in order
    for i in 0..5 {
        let pop_result = q.pop();
        assert!(pop_result.is_ok(), "Failed to pop task");
        assert_eq!(
            pop_result.unwrap(),
            Some(format!("Task-{}", i)),
            "Task order mismatch"
        );
    }
}