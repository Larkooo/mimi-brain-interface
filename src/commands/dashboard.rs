pub async fn run(host: &str, port: u16) {
    let display_host = if host == "0.0.0.0" { "localhost" } else { host };
    println!("Starting Mimi dashboard on http://{}:{}", display_host, port);
    if host == "0.0.0.0" {
        eprintln!(
            "warning: binding to 0.0.0.0 — dashboard has no auth, all interfaces are exposed"
        );
    }
    crate::dashboard::serve(host, port).await;
}
