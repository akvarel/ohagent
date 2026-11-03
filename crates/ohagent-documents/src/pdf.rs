//! Optional Linux PDF rendering in a networkless, resource-limited Podman worker.
//! No document parser runs in the API process or receives provider credentials.
use crate::{validate_request, DocumentError, ExtractRequest};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

#[derive(Clone)]
pub struct PdfRasterizer {
    image: String,
}

impl PdfRasterizer {
    /// Image must already exist locally. Rendering never pulls or builds images.
    pub fn new(image: String) -> Result<Self, DocumentError> {
        if image.is_empty()
            || image.len() > 256
            || image.starts_with('-')
            || !image
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"/.:@_-".contains(&c))
        {
            return Err(DocumentError::InvalidConfig);
        }
        Ok(Self { image })
    }

    pub async fn render(&self, request: &ExtractRequest) -> Result<Vec<String>, DocumentError> {
        validate_request(request)?;
        if request.media_type != "application/pdf" {
            return Err(DocumentError::InvalidInput);
        }
        let directory = tempfile::tempdir().map_err(|_| DocumentError::Transport)?;
        let input = directory.path().join("input.pdf");
        let output = directory.path().join("output");
        tokio::fs::create_dir(&output)
            .await
            .map_err(|_| DocumentError::Transport)?;
        let bytes = STANDARD
            .decode(&request.content_base64)
            .map_err(|_| DocumentError::InvalidInput)?;
        tokio::fs::write(&input, bytes)
            .await
            .map_err(|_| DocumentError::Transport)?;
        let info = self
            .run(&input, &output, "/usr/bin/pdfinfo", &["/input.pdf"], 5)
            .await?;
        let info = std::str::from_utf8(&info).map_err(|_| DocumentError::InvalidInput)?;
        let pages: usize = info
            .lines()
            .find_map(|line| {
                line.strip_prefix("Pages:")
                    .and_then(|s| s.trim().parse().ok())
            })
            .ok_or(DocumentError::InvalidInput)?;
        if !(1..=10).contains(&pages) {
            return Err(DocumentError::InvalidInput);
        }
        self.run(
            &input,
            &output,
            "/usr/bin/pdftoppm",
            &[
                "-png",
                "-r",
                "96",
                "-scale-to",
                "1600",
                "-f",
                "1",
                "-l",
                &pages.to_string(),
                "/input.pdf",
                "/output/page",
            ],
            15,
        )
        .await?;
        let mut files = Vec::new();
        let mut entries = tokio::fs::read_dir(&output)
            .await
            .map_err(|_| DocumentError::Transport)?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|_| DocumentError::Transport)?
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            let number = name
                .strip_prefix("page-")
                .and_then(|s| s.strip_suffix(".png"))
                .and_then(|s| s.parse::<usize>().ok())
                .ok_or(DocumentError::InvalidInput)?;
            if entry
                .file_type()
                .await
                .map_err(|_| DocumentError::Transport)?
                .is_symlink()
            {
                return Err(DocumentError::InvalidInput);
            }
            files.push((number, entry.path()));
        }
        files.sort_by_key(|(number, _)| *number);
        if files.len() != pages {
            return Err(DocumentError::InvalidInput);
        }
        let mut result = Vec::with_capacity(pages);
        let mut total = 0usize;
        for (index, (number, path)) in files.into_iter().enumerate() {
            if number != index + 1 {
                return Err(DocumentError::InvalidInput);
            }
            let metadata = tokio::fs::metadata(&path)
                .await
                .map_err(|_| DocumentError::Transport)?;
            if metadata.len() > 8 * 1024 * 1024 {
                return Err(DocumentError::InvalidInput);
            }
            let bytes = tokio::fs::read(path)
                .await
                .map_err(|_| DocumentError::Transport)?;
            total = total.saturating_add(bytes.len());
            if total > 20 * 1024 * 1024 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                return Err(DocumentError::InvalidInput);
            }
            result.push(STANDARD.encode(bytes));
        }
        Ok(result)
    }

    async fn run(
        &self,
        input: &std::path::Path,
        output: &std::path::Path,
        program: &str,
        args: &[&str],
        seconds: u64,
    ) -> Result<Vec<u8>, DocumentError> {
        let name = format!("ohagent-pdf-{}", uuid::Uuid::new_v4());
        let mut command = Command::new("/usr/bin/podman");
        command
            .args([
                "run",
                "--rm",
                "--pull=never",
                "--network=none",
                "--read-only",
                "--cap-drop=all",
                "--security-opt=no-new-privileges",
                "--pids-limit=16",
                "--memory=384m",
                "--cpus=1",
                "--ulimit",
                "fsize=8388608:8388608",
                "--tmpfs",
                "/tmp:size=32m",
            ])
            .args(["--name", &name, "--timeout", &seconds.to_string()])
            .args([
                "--volume",
                "/usr:/usr:ro",
                "--volume",
                "/lib:/lib:ro",
                "--volume",
                "/lib64:/lib64:ro",
                "--volume",
                "/etc/fonts:/etc/fonts:ro",
            ])
            .arg("--volume")
            .arg(format!("{}:/input.pdf:ro", input.display()))
            .arg("--volume")
            .arg(format!("{}:/output:rw", output.display()))
            .arg(&self.image)
            .arg(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|_| DocumentError::Transport)?;
        let mut stdout = child.stdout.take().ok_or(DocumentError::Transport)?;
        let operation = async {
            let mut bytes = Vec::new();
            (&mut stdout)
                .take(8193)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| DocumentError::Transport)?;
            if bytes.len() > 8192 {
                return Err(DocumentError::InvalidInput);
            }
            let status = child.wait().await.map_err(|_| DocumentError::Transport)?;
            if !status.success() {
                return Err(DocumentError::InvalidInput);
            }
            Ok(bytes)
        };
        let result = tokio::time::timeout(Duration::from_secs(seconds + 3), operation).await;
        if !matches!(result, Ok(Ok(_))) {
            let _ = child.kill().await;
            // Remove only this operation's uniquely named ephemeral worker.
            let _ = tokio::time::timeout(
                Duration::from_secs(5),
                Command::new("/usr/bin/podman")
                    .args(["rm", "--force", &name])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status(),
            )
            .await;
        }
        result.map_err(|_| DocumentError::Transport)?
    }
}
