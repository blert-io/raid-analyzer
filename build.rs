use std::io::Result;

fn main() -> Result<()> {
    prost_build::compile_protos(
        &["protos/event.proto", "protos/challenge_storage.proto"],
        &["protos"],
    )?;
    Ok(())
}
