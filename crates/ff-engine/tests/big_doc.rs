//! Stress test với tài liệu lớn (corpus/big-1000.pdf — 1000 trang).
//! Mỗi trang có text "Trang N"; riêng trang index 500 có thêm "ZZMARKER".

use std::path::PathBuf;
use std::time::Instant;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn fixture() -> PathBuf {
    workspace_root().join("corpus").join("big-1000.pdf")
}

fn pdfium() -> pdfium_render::prelude::Pdfium {
    if std::env::var("FOFREEXIT_PDFIUM_PATH").is_err() {
        std::env::set_var("FOFREEXIT_PDFIUM_PATH", workspace_root());
    }
    ff_engine::bind_pdfium().expect("nạp PDFium")
}

#[test]
fn big_doc_page_count() {
    let dims = ff_engine::page_dims(&pdfium(), &fixture(), None).expect("page_dims");
    assert_eq!(dims.len(), 1000, "phải có 1000 trang");
}

#[test]
fn big_doc_render_first_and_last() {
    let pdf = pdfium();
    for idx in [0u16, 999u16] {
        let t = Instant::now();
        let r = ff_engine::render::render_page(&pdf, &fixture(), idx, 600, None)
            .unwrap_or_else(|e| panic!("render trang {idx}: {e}"));
        let ms = t.elapsed().as_millis();
        // Không trắng trơn.
        let dark = r
            .image
            .to_rgba8()
            .pixels()
            .filter(|p| p.0[3] > 10 && p.0[0] < 128 && p.0[1] < 128 && p.0[2] < 128)
            .count();
        assert!(dark > 100, "trang {idx} trắng trơn ({dark} px tối)");
        // Ngưỡng rộng rãi để bắt hồi quy hiệu năng nghiêm trọng (không phải benchmark).
        assert!(ms < 5000, "render trang {idx} quá chậm: {ms} ms");
    }
}

#[test]
fn big_doc_search_single_marker() {
    let t = Instant::now();
    let hits = ff_engine::search(&pdfium(), &fixture(), "ZZMARKER", false, None).expect("search");
    let ms = t.elapsed().as_millis();
    assert_eq!(hits.len(), 1, "ZZMARKER phải có đúng 1 kết quả");
    assert_eq!(hits[0].page_index, 500, "ZZMARKER phải ở trang index 500");
    assert!(ms < 10000, "search toàn tài liệu quá chậm: {ms} ms");
}
