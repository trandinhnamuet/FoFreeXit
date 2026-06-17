//! ff — CLI của FoFreeXit. Chạy các tính năng engine không cần UI (để test).

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ff", version, about = "FoFreeXit CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// In số trang của một file PDF.
    Info {
        /// Đường dẫn file PDF.
        input: PathBuf,
        /// Mật khẩu (nếu file mã hoá).
        #[arg(long)]
        password: Option<String>,
    },
    /// Render một trang ra ảnh PNG.
    Render {
        /// Đường dẫn file PDF đầu vào.
        input: PathBuf,
        /// Đường dẫn PNG đầu ra.
        output: PathBuf,
        /// Số trang (0-based).
        #[arg(long, default_value_t = 0)]
        page: u16,
        /// Chiều rộng ảnh mục tiêu (px).
        #[arg(long, default_value_t = 1240)]
        width: u32,
        /// Mật khẩu (nếu file mã hoá).
        #[arg(long)]
        password: Option<String>,
    },
    /// In kích thước (points) của mọi trang.
    Pages {
        input: PathBuf,
        #[arg(long)]
        password: Option<String>,
    },
    /// In text trích từ một trang.
    Text {
        input: PathBuf,
        #[arg(long, default_value_t = 0)]
        page: u16,
        #[arg(long)]
        password: Option<String>,
    },
    /// Tìm chuỗi trong tài liệu; in số kết quả và vị trí.
    Search {
        input: PathBuf,
        /// Chuỗi cần tìm.
        query: String,
        /// Phân biệt hoa/thường.
        #[arg(long)]
        case_sensitive: bool,
        #[arg(long)]
        password: Option<String>,
    },
    /// In outline (bookmarks) của tài liệu.
    Outline {
        input: PathBuf,
        #[arg(long)]
        password: Option<String>,
    },
    /// (debug) In hộp bao từng ký tự của một trang.
    Chars {
        input: PathBuf,
        #[arg(long, default_value_t = 0)]
        page: u16,
        #[arg(long)]
        password: Option<String>,
    },
    /// Liệt kê annotation trong tài liệu.
    Annots { input: PathBuf },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let pdfium = ff_engine::bind_pdfium().context("khởi tạo PDFium")?;

    match cli.command {
        Command::Info { input, password } => {
            let n = ff_engine::page_count(&pdfium, &input, password.as_deref())
                .with_context(|| format!("đọc {}", input.display()))?;
            println!("{} trang: {}", input.display(), n);
        }
        Command::Render {
            input,
            output,
            page,
            width,
            password,
        } => {
            let img = ff_engine::render_page_png(
                &pdfium,
                &input,
                page,
                &output,
                width,
                password.as_deref(),
            )
            .with_context(|| format!("render {} trang {}", input.display(), page))?;
            println!(
                "Đã render trang {} ({}x{}px) -> {}",
                page,
                img.width,
                img.height,
                output.display()
            );
        }
        Command::Pages { input, password } => {
            let dims = ff_engine::page_dims(&pdfium, &input, password.as_deref())
                .with_context(|| format!("đọc {}", input.display()))?;
            for d in &dims {
                println!("trang {}: {:.1} x {:.1} pt", d.index, d.width_pt, d.height_pt);
            }
            println!("tổng: {} trang", dims.len());
        }
        Command::Text {
            input,
            page,
            password,
        } => {
            let t = ff_engine::extract_text(&pdfium, &input, page, password.as_deref())
                .with_context(|| format!("trích text {} trang {}", input.display(), page))?;
            println!("{t}");
        }
        Command::Search {
            input,
            query,
            case_sensitive,
            password,
        } => {
            let hits = ff_engine::search(&pdfium, &input, &query, case_sensitive, password.as_deref())
                .with_context(|| format!("tìm \"{query}\" trong {}", input.display()))?;
            for h in &hits {
                match h.rect {
                    Some(r) => println!(
                        "trang {} @char {} (len {}) rect [{:.0},{:.0},{:.0},{:.0}]",
                        h.page_index, h.char_start, h.char_len, r.left, r.bottom, r.right, r.top
                    ),
                    None => println!(
                        "trang {} @char {} (len {})",
                        h.page_index, h.char_start, h.char_len
                    ),
                }
            }
            println!("tổng: {} kết quả", hits.len());
        }
        Command::Outline { input, password } => {
            let items = ff_engine::outline(&pdfium, &input, password.as_deref())
                .with_context(|| format!("đọc outline {}", input.display()))?;
            for it in &items {
                let p = it
                    .page_index
                    .map(|p| format!("-> trang {p}"))
                    .unwrap_or_else(|| "(không rõ trang)".into());
                println!("{}{} {}", "  ".repeat(it.level as usize), it.title, p);
            }
            println!("tổng: {} mục", items.len());
        }
        Command::Chars {
            input,
            page,
            password,
        } => {
            let boxes = ff_engine::page_char_boxes(&pdfium, &input, page, password.as_deref())
                .with_context(|| format!("char boxes {} trang {}", input.display(), page))?;
            for (i, b) in boxes.iter().enumerate() {
                println!(
                    "{i:3} {:?} L{:.1} B{:.1} R{:.1} T{:.1}",
                    b.ch, b.left, b.bottom, b.right, b.top
                );
            }
            println!("tổng: {} ký tự", boxes.len());
        }
        Command::Annots { input } => {
            let items = ff_engine::list_annotations(&pdfium, &input)
                .with_context(|| format!("đọc annotation {}", input.display()))?;
            for a in &items {
                println!(
                    "trang {} [{}] rect [{:.0},{:.0},{:.0},{:.0}] {}",
                    a.page_index,
                    a.kind,
                    a.rect.left,
                    a.rect.bottom,
                    a.rect.right,
                    a.rect.top,
                    a.contents.as_deref().unwrap_or("")
                );
            }
            println!("tổng: {} annotation", items.len());
        }
    }
    Ok(())
}
