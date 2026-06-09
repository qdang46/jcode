/// Shared binary-extension check used by grep, symbol, and multi_grep tools.
///
/// Returns `true` when the file extension indicates a binary (non-text) format,
/// allowing callers to skip reading the file contents and avoid binary noise in
/// search results.
use std::path::Path;

pub(crate) fn is_binary_extension(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy().to_lowercase();
        let binary_exts = [
            "png", "jpg", "jpeg", "gif", "bmp", "ico", "webp", "pdf", "zip", "tar", "gz", "bz2",
            "xz", "7z", "rar", "exe", "dll", "so", "dylib", "o", "a", "class", "pyc", "wasm",
            "mp3", "mp4", "avi", "mov", "mkv", "flac", "ogg", "wav", "ttf", "woff", "woff2",
        ];
        return binary_exts.contains(&ext.as_str());
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_binary_extension() {
        assert!(is_binary_extension(Path::new("image.png")));
        assert!(is_binary_extension(Path::new("archive.zip")));
        assert!(is_binary_extension(Path::new("binary.exe")));
        assert!(!is_binary_extension(Path::new("text.rs")));
        assert!(!is_binary_extension(Path::new("README.md")));
        assert!(!is_binary_extension(Path::new("Makefile")));
    }
}
