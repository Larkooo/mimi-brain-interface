pub async fn run(port: u16) {
    println!("Starting Mimi dashboard on http://localhost:{}", port);
    crate::dashboard::serve(port).await;
}
