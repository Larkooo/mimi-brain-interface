pub async fn run(port: u16) {
    println!("Starting Mimi dashboard on http://localhost:{}", port);
    // The dashboard is the always-on process, so it also drives the schedules
    // created in the Schedules tab. `mimi cron daemon` runs the same loop if
    // you'd rather give it a dedicated unit.
    tokio::spawn(crate::crons::scheduler());
    crate::dashboard::serve(port).await;
}
