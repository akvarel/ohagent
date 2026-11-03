use base64::{engine::general_purpose::STANDARD, Engine};
use ohagent_documents::{ExtractRequest, PdfRasterizer};

fn pdf(pages: usize) -> Vec<u8> {
    let mut objects = vec!["<< /Type /Catalog /Pages 2 0 R >>".to_string()];
    let kids = (0..pages)
        .map(|i| format!("{} 0 R", 3 + i * 2))
        .collect::<Vec<_>>()
        .join(" ");
    objects.push(format!("<< /Type /Pages /Kids [{kids}] /Count {pages} >>"));
    for i in 0..pages {
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents {} 0 R /Resources << >> >>", 4 + i * 2));
        objects.push("<< /Length 0 >>\nstream\n\nendstream".to_string());
    }
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (i, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets.iter().skip(1) {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    bytes
}

#[tokio::test]
#[ignore = "Requires local Podman alpine:3.20 image and host Poppler; run explicitly for acceptance"]
async fn isolated_pdf_worker_renders_all_pages_and_rejects_excess_pages() {
    let renderer = PdfRasterizer::new("docker.io/library/alpine:3.20".into()).unwrap();
    let mut request = ExtractRequest {
        file_name: "artificial.pdf".into(),
        media_type: "application/pdf".into(),
        content_base64: STANDARD.encode(pdf(2)),
    };
    let images = renderer.render(&request).await.unwrap();
    assert_eq!(images.len(), 2);
    for image in images {
        assert!(STANDARD
            .decode(image)
            .unwrap()
            .starts_with(b"\x89PNG\r\n\x1a\n"));
    }
    request.content_base64 = STANDARD.encode(pdf(11));
    assert!(renderer.render(&request).await.is_err());
    request.content_base64 = STANDARD.encode(b"%PDF-broken");
    assert!(renderer.render(&request).await.is_err());
}

#[test]
fn rejects_untrusted_image_names() {
    assert!(PdfRasterizer::new("--privileged".into()).is_err());
    assert!(PdfRasterizer::new("image name".into()).is_err());
}
