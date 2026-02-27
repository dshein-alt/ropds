use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;

use oxc_allocator::Allocator;
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_minifier::{Minifier, MinifierOptions};
use oxc_parser::Parser;
use oxc_span::SourceType;
use walkdir::WalkDir;

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=static");
    println!("cargo:rerun-if-changed=templates");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
    let out_dir = env::var("OUT_DIR")?;
    let output_root = Path::new(&out_dir).join("embedded_assets");
    let strict_js_minify = env::var("ROPDS_STRICT_JS_MINIFY").ok().as_deref() == Some("1");

    if output_root.exists() {
        fs::remove_dir_all(&output_root)?;
    }
    fs::create_dir_all(&output_root)?;

    let _ = copy_tree(
        &Path::new(&manifest_dir).join("templates"),
        &output_root.join("templates"),
        false,
        false,
    )?;
    let stats = copy_tree(
        &Path::new(&manifest_dir).join("static"),
        &output_root.join("static"),
        true,
        strict_js_minify,
    )?;
    print_minify_report(&stats, strict_js_minify);

    Ok(())
}

fn copy_tree(
    src_root: &Path,
    dst_root: &Path,
    minify_js_files: bool,
    strict_js_minify: bool,
) -> io::Result<MinifyStats> {
    let mut stats = MinifyStats::default();

    for entry in WalkDir::new(src_root) {
        let entry = entry?;
        let src = entry.path();
        let relative = src.strip_prefix(src_root).map_err(io::Error::other)?;
        let dst = dst_root.join(relative);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&dst)?;
            continue;
        }

        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }

        let should_minify = minify_js_files
            && src
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("js"));

        if should_minify {
            let source = fs::read(src)?;
            let source_len = source.len() as u64;
            stats.js_total_files += 1;
            stats.original_js_bytes += source_len;

            if is_minification_excluded(src) {
                stats.js_excluded_files += 1;
                stats.generated_js_bytes += source_len;
                fs::write(&dst, source)?;
                continue;
            }
            match minify_javascript(src, &source) {
                Ok(output) => {
                    let output_len = output.len() as u64;
                    stats.js_minified_files += 1;
                    stats.minified_input_bytes += source_len;
                    stats.minified_output_bytes += output_len;
                    stats.generated_js_bytes += output_len;
                    fs::write(&dst, output)?
                }
                Err(error) => {
                    if strict_js_minify {
                        return Err(error);
                    }
                    println!(
                        "cargo:warning=JS minification skipped for {} ({})",
                        src.display(),
                        error
                    );
                    stats.js_fallback_files += 1;
                    stats.generated_js_bytes += source_len;
                    fs::write(&dst, source)?;
                }
            }
        } else {
            fs::copy(src, &dst)?;
        }
    }

    Ok(stats)
}

#[derive(Default)]
struct MinifyStats {
    js_total_files: u64,
    js_minified_files: u64,
    js_excluded_files: u64,
    js_fallback_files: u64,
    original_js_bytes: u64,
    generated_js_bytes: u64,
    minified_input_bytes: u64,
    minified_output_bytes: u64,
}

fn print_minify_report(stats: &MinifyStats, strict: bool) {
    if stats.js_total_files == 0 {
        println!("cargo:warning=JS minify report: no JS files found");
        return;
    }

    let total_saved = stats
        .original_js_bytes
        .saturating_sub(stats.generated_js_bytes);
    let total_saved_pct = percent(total_saved, stats.original_js_bytes);

    let minified_saved = stats
        .minified_input_bytes
        .saturating_sub(stats.minified_output_bytes);
    let minified_saved_pct = percent(minified_saved, stats.minified_input_bytes);

    println!(
        "cargo:warning=JS minify report: strict={}, total={}, minified={}, excluded={}, fallback={}",
        strict,
        stats.js_total_files,
        stats.js_minified_files,
        stats.js_excluded_files,
        stats.js_fallback_files,
    );
    println!(
        "cargo:warning=JS bytes: total {} -> {} (saved {} / {:.1}%), minified subset {} -> {} (saved {} / {:.1}%)",
        stats.original_js_bytes,
        stats.generated_js_bytes,
        total_saved,
        total_saved_pct,
        stats.minified_input_bytes,
        stats.minified_output_bytes,
        minified_saved,
        minified_saved_pct,
    );
}

fn percent(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        (numerator as f64) * 100.0 / (denominator as f64)
    }
}

fn is_minification_excluded(path: &Path) -> bool {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".min.js"))
    {
        return true;
    }

    let normalized = path.to_string_lossy().replace('\\', "/");
    normalized.contains("/static/lib/foliate/vendor/")
}

fn minify_javascript(path: &Path, source: &[u8]) -> io::Result<Vec<u8>> {
    let source_text = std::str::from_utf8(source)
        .map_err(|e| io::Error::other(format!("{} is not UTF-8: {e}", path.display())))?;

    let preferred = preferred_source_type(path);
    let fallback = if preferred.is_module() {
        SourceType::default().with_script(true)
    } else {
        SourceType::default().with_module(true)
    };

    for source_type in [preferred, fallback] {
        if let Ok(output) = minify_with_source_type(source_text, source_type) {
            return Ok(output.into_bytes());
        }
    }

    Err(io::Error::other(format!(
        "failed to minify {} in both parser modes",
        path.display()
    )))
}

fn preferred_source_type(path: &Path) -> SourceType {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if normalized.contains("/static/lib/foliate/") {
        SourceType::default().with_module(true)
    } else {
        SourceType::default().with_script(true)
    }
}

fn minify_with_source_type(source_text: &str, source_type: SourceType) -> io::Result<String> {
    let allocator = Allocator::default();
    let parser_return = Parser::new(&allocator, source_text, source_type).parse();
    if !parser_return.errors.is_empty() {
        return Err(io::Error::other("parse error"));
    }
    let mut program = parser_return.program;

    let minify_return = Minifier::new(MinifierOptions::default()).minify(&allocator, &mut program);

    let codegen_options = CodegenOptions {
        minify: true,
        ..CodegenOptions::default()
    };

    let output = Codegen::new()
        .with_options(codegen_options)
        .with_scoping(minify_return.scoping)
        .with_private_member_mappings(minify_return.class_private_mappings)
        .build(&program)
        .code;

    Ok(output)
}
