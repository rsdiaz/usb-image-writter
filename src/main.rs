use clap::{command, Args, Parser, Subcommand};
use std::io::{self, Error, Write};

#[cfg(target_os = "windows")]
mod win32;
extern crate winapi;

use win32::{list_physical_disks, list_volumes, open_device, write_to_device};
use windows::Win32::Foundation::CloseHandle;

/// Simple program to greet a person
#[derive(Parser)]
#[command(version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    /// List all physical disks
    List,
    /// Flashing image to disk
    Flash(AddArgs),
}
#[derive(Args)]
struct AddArgs {
    image_file_path: String,
    device_path: String,
}

fn main() -> Result<(), Error> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::List => {
            let _ = list_physical_disks();
            let _ = list_volumes();
        }
        Commands::Flash(args) => {
            println!("Image path: {:?}", args.image_file_path);
            println!("Device path: {:?}", args.device_path);


            // let image_path = r#"C:\Users\gamer\Downloads\image\2024-03-15-raspios-bookworm-armhf-lite.img"#;
            // let usb_device_path = r#"\\.\PhysicalDrive4"#;

            let handle = open_device(&args.device_path)?;

            // Escribir la imagen en el dispositivo USB
            write_to_device(handle, &args.image_file_path, 1024 * 1024, |written, total| {
                let percentage = (written as f64 / total as f64) * 100.0;
                print!(
                    "\rProgreso: {:.2}% ({} de {} bytes escritos)",
                    percentage, written, total
                );
                io::stdout().flush().unwrap();
            })?;

            unsafe {
                let _ = CloseHandle(handle);
            }
        
        }
    }

    Ok(())
}
