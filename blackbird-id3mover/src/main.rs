use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use clap::Parser;
use lofty::{file::TaggedFileExt, read_from_path};
use sanitize_filename::sanitize;
use walkdir::WalkDir;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Directory containing music files to organize
    directory: PathBuf,

    /// Show what would be moved without actually moving files
    #[arg(long)]
    dry_run: bool,

    /// Copy files instead of moving them
    #[arg(long)]
    copy: bool,

    /// Show verbose output with all file operations
    #[arg(short, long)]
    verbose: bool,

    /// Write file operation report to the specified file
    #[arg(long)]
    output_report: Option<PathBuf>,
}

fn main() {
    let args = Args::parse();

    if !args.directory.exists() {
        eprintln!(
            "Error: Directory '{}' does not exist",
            args.directory.display()
        );
        std::process::exit(1);
    }

    let output_dir = args.directory.join("output");

    let operation = if args.copy { "Copying" } else { "Moving" };
    let operation_lower = if args.copy { "copying" } else { "moving" };

    if args.dry_run {
        println!("DRY RUN MODE - No files will be {operation_lower}");
        println!("Output directory: {}", output_dir.display());
        println!();
    } else {
        println!("{operation} files to: {}", output_dir.display());
        println!();
    }

    // Open report file if specified
    let mut report_file = if let Some(ref report_path) = args.output_report {
        match fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(report_path)
        {
            Ok(file) => {
                println!("Writing report to: {}", report_path.display());
                println!();
                Some(file)
            }
            Err(e) => {
                eprintln!(
                    "Error: Failed to create report file '{}': {e}",
                    report_path.display()
                );
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    let music_extensions: HashSet<&str> = ["mp3", "flac", "m4a", "aac", "ogg", "wav", "wma", "mp4"]
        .iter()
        .cloned()
        .collect();

    match process_directory(
        &args.directory,
        &output_dir,
        &music_extensions,
        args.dry_run,
        args.copy,
        args.verbose,
        &mut report_file,
    ) {
        Ok(count) => {
            if args.dry_run {
                println!("\nDry run complete. {count} files would be processed.");
            } else {
                let operation_past = if args.copy { "copied" } else { "moved" };
                println!("\nProcessing complete. {count} files {operation_past}.");
            }
        }
        Err(e) => {
            eprintln!("Error: {e:?}");
            std::process::exit(1);
        }
    }
}

fn process_directory(
    input_dir: &Path,
    output_dir: &Path,
    music_extensions: &HashSet<&str>,
    dry_run: bool,
    copy: bool,
    verbose: bool,
    report_file: &mut Option<fs::File>,
) -> Result<usize> {
    let mut processed_count = 0;

    for entry in WalkDir::new(input_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let file_path = entry.path();

        // Skip files in the output directory to avoid moving already processed files
        if file_path.starts_with(output_dir) {
            continue;
        }

        // Check if it's a music file
        let Some(extension) = file_path.extension() else {
            continue;
        };
        let Some(ext_str) = extension.to_str() else {
            continue;
        };
        if !music_extensions.contains(ext_str.to_lowercase().as_str()) {
            continue;
        }
        match process_music_file(
            file_path,
            input_dir,
            output_dir,
            dry_run,
            copy,
            verbose,
            report_file,
        ) {
            Ok(true) => processed_count += 1,
            Ok(false) => {} // File skipped (missing tags)
            Err(e) => {
                eprintln!("Error: Failed to process {}: {e:?}", file_path.display())
            }
        }
    }

    Ok(processed_count)
}

fn process_music_file(
    file_path: &Path,
    input_dir: &Path,
    output_dir: &Path,
    dry_run: bool,
    copy: bool,
    verbose: bool,
    report_file: &mut Option<fs::File>,
) -> Result<bool> {
    // Display the file path for error messages
    let file_path_display = file_path.display();

    // Read metadata using Lofty
    let metadata = read_metadata_with_lofty(file_path)
        .with_context(|| format!("Failed to read metadata from {file_path_display}"))?;

    // Extract required tags
    let album_artist = if let Some(album_artist) = &metadata.album_artist {
        album_artist
    } else if let Some(artist) = &metadata.artist {
        eprintln!(
            "Warning: No album artist tag found in {file_path_display}, using artist tag instead"
        );
        artist
    } else {
        return Err(anyhow::anyhow!(
            "Missing both album artist and artist tags in {file_path_display}"
        ));
    };

    let track_title = metadata
        .title
        .as_ref()
        .with_context(|| format!("Missing title tag in {file_path_display}"))?;

    let album = metadata.album.as_ref().unwrap_or_else(|| {
        eprintln!("Warning: No album tag found in {file_path_display}, using track title instead");
        track_title
    });

    // Get track number (pad to 2 digits)
    let track_num = metadata
        .track_number
        .map(|t| format!("{t:02}"))
        .unwrap_or_else(|| "00".to_string());

    // Get file extension
    let file_extension = file_path
        .extension()
        .and_then(|ext| ext.to_str())
        .with_context(|| format!("Missing file extension for {file_path_display}"))?;

    // Build filename with optional disc number
    let filename = if let Some(disc_num) = metadata.disc_number {
        format!("{track_num} - {track_title} [{disc_num}].{file_extension}")
    } else {
        format!("{track_num} - {track_title}.{file_extension}")
    };

    // Build target path with sanitized names
    let target_dir = output_dir
        .join(sanitize(album_artist))
        .join(sanitize(album));

    let target_path = target_dir.join(sanitize(&filename));
    let target_path_display = target_path.display();

    // Format the movement report
    let operation_arrow = if copy { "=>" } else { "->" };
    let report_line = format!(
        "{} {} {}",
        file_path
            .strip_prefix(input_dir)
            .unwrap_or(file_path)
            .display(),
        operation_arrow,
        target_path
            .strip_prefix(output_dir)
            .unwrap_or(&target_path)
            .display()
    );

    // Print to console
    if verbose {
        println!("{report_line}");
    }

    // Write to report file if specified
    if let Some(file) = report_file {
        writeln!(file, "{report_line}").with_context(|| "Failed to write to report file")?;
    }

    if !dry_run {
        // Create target directory
        fs::create_dir_all(&target_dir)
            .with_context(|| format!("Failed to create directory {target_dir:?}"))?;

        // Copy or move the file
        if copy {
            fs::copy(file_path, &target_path).with_context(|| {
                format!("Failed to copy {file_path_display} to {target_path_display}")
            })?;
        } else {
            fs::rename(file_path, &target_path).with_context(|| {
                format!("Failed to move {file_path_display} to {target_path_display}")
            })?;
        }
    }

    Ok(true)
}

#[derive(Debug)]
struct AudioMetadata {
    album_artist: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    title: Option<String>,
    track_number: Option<u32>,
    disc_number: Option<u32>,
}

fn read_metadata_with_lofty(file_path: &Path) -> Result<AudioMetadata> {
    // Read the file using lofty
    let tagged_file = read_from_path(file_path)
        .with_context(|| format!("Failed to read file: {}", file_path.display()))?;

    // Get the primary tag or first available tag
    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())
        .with_context(|| format!("No tags found in file: {}", file_path.display()))?;

    // Extract metadata using a more direct approach
    let mut metadata = AudioMetadata {
        album_artist: None,
        artist: None,
        album: None,
        title: None,
        track_number: None,
        disc_number: None,
    };

    // Try to get basic metadata using common tag names
    for item in tag.items() {
        let key_str = format!("{:?}", item.key()).to_lowercase();
        let value = item.value().text().unwrap_or("").trim();

        if value.is_empty() {
            continue;
        }

        match key_str.as_str() {
            k if k.contains("albumartist") || k.contains("album_artist") => {
                metadata.album_artist = Some(value.to_string());
            }
            k if k.contains("artist") && !k.contains("album") => {
                metadata.artist = Some(value.to_string());
            }
            k if k.contains("album") && !k.contains("artist") => {
                metadata.album = Some(value.to_string());
            }
            k if k.contains("title") && !k.contains("album") => {
                metadata.title = Some(value.to_string());
            }
            k if k.contains("track") => {
                if let Ok(track_num) = value.parse::<u32>() {
                    metadata.track_number = Some(track_num);
                }
            }
            k if k.contains("disc") => {
                if let Ok(disc_num) = value.parse::<u32>() {
                    metadata.disc_number = Some(disc_num);
                }
            }
            _ => {}
        }
    }

    Ok(metadata)
}
