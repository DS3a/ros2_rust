#![warn(missing_docs)]
//! Rust client library for ROS 2.
//!
//! For getting started, see the [README][1].
//!
//! [1]: https://github.com/ros2-rust/ros2_rust/blob/main/README.md

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

mod context;
mod error;
mod future;
mod node;
mod qos;
mod wait;

mod rcl_bindings;

pub use context::*;
pub use error::*;
pub use node::*;
pub use qos::*;
pub use wait::*;

use rcl_bindings::rcl_context_is_valid;

pub use rcl_bindings::rmw_request_id_t;

use parking_lot::Mutex;

/// Polls the node for new messages and executes the corresponding callbacks.
///
/// See [`WaitSet::wait`] for the meaning of the `timeout` parameter.
///
/// This may under some circumstances return
/// [`SubscriptionTakeFailed`][1], [`ClientTakeFailed`][1], [`ServiceTakeFailed`][1] when the wait
/// set spuriously wakes up.
/// This can usually be ignored.
///
/// [1]: crate::RclReturnCode
pub fn spin_once(node: &Node, timeout: Option<Duration>) -> Result<(), RclrsError> {
    let live_subscriptions = node.live_subscriptions();
    let live_clients = node.live_clients();
    let live_services = node.live_services();
    let ctx = Context {
        rcl_context_mtx: node.rcl_context_mtx.clone(),
    };
    let mut wait_set = WaitSet::new(
        live_subscriptions.len(),
        0,
        0,
        live_clients.len(),
        live_services.len(),
        0,
        &ctx,
    )?;

    for live_subscription in &live_subscriptions {
        wait_set.add_subscription(live_subscription.clone())?;
    }

    for live_client in &live_clients {
        wait_set.add_client(live_client.clone())?;
    }

    for live_service in &live_services {
        wait_set.add_service(live_service.clone())?;
    }

    let ready_entities = wait_set.wait(timeout)?;

    for ready_subscription in ready_entities.subscriptions {
        ready_subscription.execute()?;
    }

    for ready_client in ready_entities.clients {
        ready_client.execute()?;
    }

    for ready_service in ready_entities.services {
        ready_service.execute()?;
    }

    Ok(())
}

/// Convenience function for calling [`rclrs::spin_once`] in a loop.
///
/// This function additionally checks that the context is still valid.
pub fn spin(node: &Node) -> Result<(), RclrsError> {
    // The context_is_valid functions exists only to abstract away ROS distro differences
    #[cfg(ros_distro = "foxy")]
    // SAFETY: No preconditions for this function.
    let context_is_valid = || unsafe { rcl_context_is_valid(&mut *node.rcl_context_mtx.lock()) };
    #[cfg(not(ros_distro = "foxy"))]
    // SAFETY: No preconditions for this function.
    let context_is_valid = || unsafe { rcl_context_is_valid(&*node.rcl_context_mtx.lock()) };

    while context_is_valid() {
        match spin_once(node, None) {
            Ok(_)
            | Err(RclrsError::RclError {
                code: RclReturnCode::Timeout,
                ..
            }) => (),
            error => return error,
        }
    }
    Ok(())
}

/// Convenience function for calling [`rclrs::spin_once`] in a loop.
///
/// This function additionally checks that the context is still valid.
pub fn spin_some(node: &Node) -> Result<(), RclrsError> {
    // The context_is_valid functions exists only to abstract away ROS distro differences
    #[cfg(ros_distro = "foxy")]
    // SAFETY: No preconditions for this function.
    let context_is_valid = || unsafe { rcl_context_is_valid(&mut *node.context.lock()) };
    #[cfg(not(ros_distro = "foxy"))]
    // SAFETY: No preconditions for this function.
    let context_is_valid = || unsafe { rcl_context_is_valid(&*node.context.lock()) };

    if context_is_valid() {
        if let Some(error) = spin_once(node, Some(std::time::Duration::from_millis(500))).err() {
            match error.code {
                RclReturnCode::Timeout => (),
                _ => return Err(error),
            }
        }
    }
    Ok(())
}

pub fn spin_until_future_complete<T: Unpin + Clone>(
    node: &node::Node,
    future: Arc<Mutex<Box<crate::future::RclFuture<T>>>>,
) -> Result<<future::RclFuture<T> as Future>::Output, RclrsError> {
    let rclwaker = Arc::new(crate::future::RclWaker {});
    let waker = crate::future::rclwaker_into_waker(Arc::into_raw(rclwaker));
    let mut cx = std::task::Context::from_waker(&waker);

    loop {
        let context_valid = unsafe { rcl_context_is_valid(&mut *node.context.lock()) };
        if context_valid {
            if let Some(error) = spin_once(node, None).err() {
                match error {
                    RclrsError {
                        code: RclReturnCode::Timeout,
                        ..
                    } => continue,
                    error => return Err(error),
                };
            };
            match Future::poll(Pin::new(&mut *future.lock()), &mut cx) {
                Poll::Ready(val) => break Ok(val),
                Poll::Pending => continue,
            };
        }
    }
}
