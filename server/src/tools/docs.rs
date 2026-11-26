//! Document processing: PDF, DOCX, PPTX, XLSX/XLS/ODS, HTML, CSV, text.
//! PDF text uses pdftotext when available, with a pure-Rust fallback.
//! LibreOffice handles conversions; tesseract handles image OCR.

use super::{arg_str, arg_u32, files, find_binary, truncate_chars, ToolOutput};
use crate::state::AppState;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

pub fn capabilities(state: &AppState) -> Value {
    let cfg = state.config.get();
    json!({
        "pdftotext": find_binary(&cfg.docs.pdftotext_path, &["pdftotext"]).map(|p| p.to_string_lossy().to_string()),
        "pdfinfo": find_binary("", &["pdfinfo"]).map(|p| p.to_string_lossy().to_string()),
        "soffice": find_binary(&cfg.docs.soffice_path, &["soffice", "libreoffice", "soffice.exe"]).map(|p| p.to_string_lossy().to_string()),
        "tesseract": find_binary(&cfg.docs.tesseract_path, &["tesseract"]).map(|p| p.to_string_lossy().to_string()),
        "max_chars": cfg.docs.max_chars,
    })
}

fn ext_lower(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn kind(path: &Path) -> &'static str {
    match ext_lower(path).as_str() {
        "pdf" => "pdf",
        "docx" => "docx",
        "doc" => "doc",
        "pptx" => "pptx",
        "ppt" => "ppt",
        "xlsx" => "xlsx",
        "xls" => "xls",
        "ods" => "ods",
        "csv" => "csv",
        "tsv" => "tsv",
        "md" | "markdown" => "markdown",
        "html" | "htm" => "html",
        "json" => "json",
        "xml" => "xml",
        "txt" | "log" | "rst" | "yaml" | "yml" | "toml" | "ini" => "text",
        _ => "unknown",
    }
}

fn unescape_xml(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Extracts the text nodes of a WordprocessingML/SlidesML document.
fn xml_text(xml: &str, open_tag: &str, close_tag: &str, para_tag: &str, cell_tag: &str) -> String {
    let mut out = String::new();
    let mut in_text = false;
    let mut i = 0usize;
    while let Some(rel) = xml[i..].find('<') {
        let lt = i + rel;
        if in_text {
            out.push_str(&xml[i..lt]);
        }
        let Some(gt_rel) = xml[lt..].find('>') else { break };
        let gt = lt + gt_rel;
        let tag = xml[lt + 1..gt].trim();
        let closing = tag.starts_with('/');
        let self_closing = tag.ends_with('/');
        let name_part = if closing { &tag[1..] } else { tag };
        let name = name_part
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("");
        if !closing && name == open_tag && !self_closing {
            in_text = true;
        } else if closing && name == close_tag {
            in_text = false;
        }
        if closing && name == para_tag {
            out.push('\n');
        }
        if closing && !cell_tag.is_empty() && name == cell_tag {
            out.push('\t');
        }
        if name == "w:br" || name == "a:br" {
            out.push('\n');
        }
        if name == "w:tab" || name == "a:tab" {
            out.push('\t');
        }
        i = gt + 1;
    }
    let decoded = unescape_xml(&out);
    let mut result = String::new();
    let mut blanks = 0;
    for line in decoded.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blanks += 1;
            if blanks > 1 {
                continue;
            }
        } else {
            blanks = 0;
        }
        result.push_str(line);
        result.push('\n');
    }
    result.trim().to_string()
}

fn zip_entry(path: &Path, name: &str) -> Result<String, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("not a valid Office archive: {e}"))?;
    let mut entry = zip
        .by_name(name)
        .map_err(|_| format!("archive has no {name} (not a valid document?)"))?;
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|e| format!("cannot read {name}: {e}"))?;
    Ok(text)
}

fn docx_text(path: &Path) -> Result<String, String> {
    let xml = zip_entry(path, "word/document.xml")?;
    Ok(xml_text(&xml, "w:t", "w:t", "w:p", "w:tc"))
}

fn slide_number(name: &str) -> u32 {
    name.trim_start_matches("ppt/slides/slide")
        .trim_end_matches(".xml")
        .parse()
        .unwrap_or(0)
}

fn pptx_text(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("not a valid pptx: {e}"))?;
    let mut slides: Vec<String> = zip
        .file_names()
        .filter_map(|n| n.ok().map(|c| c.to_string()))
        .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
        .collect();
    slides.sort_by_key(|n| slide_number(n));
    if slides.is_empty() {
        return Err("pptx has no slides".into());
    }
    let mut out = String::new();
    for name in slides {
        let mut xml = String::new();
        {
            use std::io::Read;
            let mut entry = zip.by_name(&name).map_err(|e| format!("cannot read {name}: {e}"))?;
            entry.read_to_string(&mut xml).map_err(|e| format!("cannot read {name}: {e}"))?;
        }
        out.push_str(&format!("\n--- Slide {} ---\n", slide_number(&name)));
        out.push_str(&xml_text(&xml, "a:t", "a:t", "a:p", ""));
        out.push('\n');
    }
    Ok(out.trim().to_string())
}

fn cell_text(c: &calamine::Data) -> String {
    use calamine::Data;
    match c {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) => {
            if f.fract() == 0.0 {
                format!("{f:.0}")
            } else {
                format!("{f}")
            }
        }
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(d) => d.to_string(),
        Data::Error(e) => format!("{e:?}"),
        _ => String::new(),
    }
}

fn sheets_text(path: &Path) -> Result<String, String> {
    use calamine::{open_workbook_auto, Reader};
    let mut wb = open_workbook_auto(path).map_err(|e| format!("cannot open spreadsheet: {e}"))?;
    let names = wb.sheet_names().to_vec();
    let mut out = String::new();
    for (i, name) in names.iter().enumerate() {
        out.push_str(&format!("## Sheet: {name}\n"));
        match wb.worksheet_range(name) {
            Ok(range) => {
                let mut rows = 0;
                for row in range.rows() {
                    let line: Vec<String> = row.iter().map(cell_text).collect();
                    out.push_str(line.join("\t").trim_end());
                    out.push('\n');
                    rows += 1;
                    if rows >= 5000 {
                        out.push_str("(sheet truncated at 5000 rows)\n");
                        break;
                    }
                }
            }
            Err(e) => out.push_str(&format!("(cannot read sheet: {e})\n")),
        }
        if i + 1 < names.len() {
            out.push('\n');
        }
    }
    Ok(out.trim_end().to_string())
}

async fn pdf_text(state: &AppState, path: &Path) -> Result<String, String> {
    let cfg = state.config.get();
    if let Some(bin) = find_binary(&cfg.docs.pdftotext_path, &["pdftotext"]) {
        let out = tokio::process::Command::new(bin)
            .arg("-layout")
            .arg(path)
            .arg("-")
            .output()
            .await
            .map_err(|e| format!("pdftotext failed: {e}"))?;
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).to_string());
        }
    }
    let p = path.to_path_buf();
    tokio::task::spawn_blocking(move || pdf_extract::extract_text(&p).map_err(|e| format!("PDF extraction failed: {e}")))
        .await
        .map_err(|e| format!("PDF task panicked: {e}"))?
}

fn read_text_file(path: &Path, cap: u64) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("cannot stat {}: {e}", path.display()))?;
    if meta.len() > cap {
        return Err(format!(
            "file is {} bytes, above the {cap}-byte text limit (use read_file with base64 for binaries)",
            meta.len()
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

pub async fn read(state: Arc<AppState>, args: Value) -> Result<ToolOutput, String> {
    let raw = arg_str(&args, "path").ok_or("path is required")?;
    let path = files::resolve(&state, raw, false)?;
    let cfg = state.config.get();
    let max = arg_u32(&args, "max_chars")
        .map(|m| m as usize)
        .unwrap_or(cfg.docs.max_chars)
        .clamp(100, 5_000_000);
    let kind = kind(&path);
    let format_arg = arg_str(&args, "format").unwrap_or("text");

    let extracted: String = match kind {
        "pdf" => pdf_text(&state, &path).await?,
        "docx" => {
            let p = path.clone();
            tokio::task::spawn_blocking(move || docx_text(&p))
                .await
                .map_err(|e| format!("docx task panicked: {e}"))??
        }
        "pptx" => {
            let p = path.clone();
            tokio::task::spawn_blocking(move || pptx_text(&p))
                .await
                .map_err(|e| format!("pptx task panicked: {e}"))??
        }
        "xlsx" | "xls" | "ods" => {
            let p = path.clone();
            tokio::task::spawn_blocking(move || sheets_text(&p))
                .await
                .map_err(|e| format!("spreadsheet task panicked: {e}"))??
        }
        "doc" | "ppt" => {
            return Err(format!(
                "legacy .{kind} files are not supported directly; convert with pc_doc_convert (LibreOffice) first"
            ))
        }
        "html" => {
            let raw_html = read_text_file(&path, 50 * 1024 * 1024)?;
            if format_arg == "html" || format_arg == "raw" {
                raw_html
            } else {
                super::html_to_text(&raw_html)
            }
        }
        "csv" | "tsv" | "markdown" | "json" | "xml" | "text" => read_text_file(&path, 50 * 1024 * 1024)?,
        _ => {
            return Err(format!(
                "unsupported document type `{kind}` for {} (supported: pdf, docx, pptx, xlsx/xls/ods, doc/ppt via convert, html, csv, markdown, json, xml, text)",
                path.display()
            ))
        }
    };

    let (content, truncated) = truncate_chars(&extracted, max);
    let mut out = json!({
        "path": path.to_string_lossy(),
        "kind": kind,
        "chars": extracted.chars().count(),
        "truncated": truncated,
        "content": content,
    });
    if kind == "pdf" && extracted.trim().is_empty() {
        out["note"] = json!("no extractable text: the PDF may be scanned. Try pc_doc_ocr on rendered pages or pdftoppm.");
    }
    Ok(ToolOutput::json(out))
}

pub async fn info(state: Arc<AppState>, args: Value) -> Result<ToolOutput, String> {
    let raw = arg_str(&args, "path").ok_or("path is required")?;
    let path = files::resolve(&state, raw, false)?;
    let meta = std::fs::metadata(&path).map_err(|e| format!("cannot stat {}: {e}", path.display()))?;
    let kind = kind(&path);
    let mut out = json!({
        "path": path.to_string_lossy(),
        "kind": kind,
        "size": meta.len(),
        "modified_ms": meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64),
    });
    if kind == "pdf" {
        if let Some(pdfinfo) = find_binary("", &["pdfinfo"]) {
            if let Ok(result) = tokio::time::timeout(
                Duration::from_secs(10),
                tokio::process::Command::new(pdfinfo).arg(&path).output(),
            )
            .await
            {
                if let Ok(result) = result {
                    let text = String::from_utf8_lossy(&result.stdout);
                    for line in text.lines() {
                        if let Some(rest) = line.strip_prefix("Pages:") {
                            if let Ok(pages) = rest.trim().parse::<u32>() {
                                out["pages"] = json!(pages);
                            }
                        }
                    }
                }
            }
        }
    }
    if matches!(kind, "docx" | "pptx" | "xlsx") {
        let p = path.clone();
        if let Ok(Ok(entries)) = tokio::task::spawn_blocking(move || {
            std::fs::File::open(&p)
                .ok()
                .and_then(|f| zip::ZipArchive::new(f).ok())
                .map(|z| z.len())
                .ok_or_else(|| "not a zip archive".to_string())
        })
        .await
        {
            out["archive_entries"] = json!(entries);
        }
    }
    Ok(ToolOutput::json(out))
}

pub async fn convert(state: Arc<AppState>, args: Value) -> Result<ToolOutput, String> {
    let cfg = state.config.get();
    let input_raw = arg_str(&args, "input").ok_or("input is required")?;
    let output_raw = arg_str(&args, "output").ok_or("output is required")?;
    let input = files::resolve(&state, input_raw, false)?;
    let output = files::resolve(&state, output_raw, true)?;
    let format = arg_str(&args, "format")
        .map(|f| f.trim_start_matches('.').to_ascii_lowercase())
        .unwrap_or_else(|| ext_lower(&output));
    if format.is_empty() {
        return Err("specify the target format (e.g. pdf, docx, txt, csv, html) or an output extension".into());
    }
    let soffice = find_binary(&cfg.docs.soffice_path, &["soffice", "libreoffice", "soffice.exe"])
        .ok_or_else(|| "LibreOffice (soffice) was not found; install LibreOffice or set docs.soffice_path".to_string())?;
    let outdir = output
        .parent()
        .ok_or("output path has no parent directory")?
        .to_path_buf();
    std::fs::create_dir_all(&outdir).map_err(|e| format!("cannot create {}: {e}", outdir.display()))?;
    let profile = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(crate::config::APP_DIR)
        .join("soffice-profile");
    std::fs::create_dir_all(&profile).map_err(|e| format!("cannot create {}: {e}", profile.display()))?;

    let mut cmd = tokio::process::Command::new(&soffice);
    cmd.args(["--headless", "--norestore", "--invisible", "--nolockcheck", "--nodefault"])
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .arg("--convert-to")
        .arg(&format)
        .arg("--outdir")
        .arg(&outdir)
        .arg(&input)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let child = cmd.spawn().map_err(|e| format!("cannot start LibreOffice: {e}"))?;
    let result = tokio::time::timeout(Duration::from_secs(180), child.wait_with_output())
        .await
        .map_err(|_| "LibreOffice conversion timed out after 180 s".to_string())?
        .map_err(|e| format!("LibreOffice failed: {e}"))?;
    if !result.status.success() {
        return Err(format!(
            "LibreOffice conversion failed: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "output".into());
    let produced = outdir.join(format!("{stem}.{format}"));
    if produced != output {
        if produced.exists() {
            std::fs::rename(&produced, &output)
                .map_err(|e| format!("cannot move {} to {}: {e}", produced.display(), output.display()))?;
        } else {
            return Err(format!(
                "LibreOffice reported success but {} was not created (LibreOffice output: {})",
                produced.display(),
                String::from_utf8_lossy(&result.stdout).trim()
            ));
        }
    }
    let size = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);
    Ok(ToolOutput::json(json!({
        "ok": true,
        "input": input.to_string_lossy(),
        "output": output.to_string_lossy(),
        "format": format,
        "bytes": size,
    })))
}

pub async fn ocr(state: Arc<AppState>, args: Value) -> Result<ToolOutput, String> {
    let cfg = state.config.get();
    let raw = arg_str(&args, "path").ok_or("path is required")?;
    let path = files::resolve(&state, raw, false)?;
    let ext = ext_lower(&path);
    if !["png", "jpg", "jpeg", "tif", "tiff", "bmp", "webp"].contains(&ext.as_str()) {
        return Err(format!("OCR works on image files (png/jpg/tiff/bmp/webp), got .{ext}; render PDF pages first"));
    }
    let tesseract = find_binary(&cfg.docs.tesseract_path, &["tesseract"])
        .ok_or_else(|| "tesseract was not found; install tesseract-ocr or set docs.tesseract_path".to_string())?;
    let lang = arg_str(&args, "lang").unwrap_or("eng");
    let out = tokio::time::timeout(
        Duration::from_secs(180),
        tokio::process::Command::new(tesseract)
            .arg(&path)
            .arg("stdout")
            .arg("-l")
            .arg(lang)
            .output(),
    )
    .await
    .map_err(|_| "OCR timed out after 180 s".to_string())?
    .map_err(|e| format!("tesseract failed: {e}"))?;
    if !out.status.success() {
        return Err(format!("tesseract failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let max = arg_u32(&args, "max_chars").map(|m| m as usize).unwrap_or(cfg.docs.max_chars);
    let (content, truncated) = truncate_chars(&text, max.clamp(100, 5_000_000));
    Ok(ToolOutput::json(json!({
        "path": path.to_string_lossy(),
        "lang": lang,
        "chars": text.chars().count(),
        "truncated": truncated,
        "content": content,
    })))
}

