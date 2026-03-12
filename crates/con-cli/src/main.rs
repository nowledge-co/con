use anyhow::Result;

fn main() -> Result<()> {
    println!("con-cli: Socket client for con terminal");
    println!("Usage: con-cli <command>");
    println!();
    println!("Commands:");
    println!("  notify <title> [body]    Send a notification to con");
    println!("  context                  Get current terminal context");
    println!("  capabilities             List available socket API methods");
    println!();
    println!("Socket path: /tmp/con.sock");
    println!();
    println!("(Socket API not yet implemented — coming in Phase 2)");
    Ok(())
}
