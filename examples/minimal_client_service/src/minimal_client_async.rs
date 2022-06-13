use anyhow::{Error, Result};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let context = rclrs::Context::new(env::args()).unwrap();

    let mut node = context.create_node("minimal_client")?;

    let client = node.create_client::<example_interfaces::srv::AddTwoInts>("add_two_ints")?;

    println!("Starting client");

    std::thread::sleep(std::time::Duration::from_millis(500));

    let request = example_interfaces::srv::AddTwoInts_Request { a: 41, b: 1 };

    let future = client.call_async(&request);

    println!("Waiting for response");

    let spin_thread = std::thread::spawn(move || rclrs::spin(&node).map_err(|err| err));

    let response = future.await;
    println!(
        "Result of {} + {} is: {}",
        request.a,
        request.b,
        response.unwrap().sum
    );

    spin_thread.join().ok();
    Ok(())
}
