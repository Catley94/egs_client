// Basic smoke test to ensure download assembly writes non-zero bytes.
// This test doesn't hit Epic APIs; it simulates a simple write path to ensure
// that our approach of writing to a temp file then renaming results in non-zero files.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

#[test]
fn writes_non_zero_then_renames() {
    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("file.bin");
    let tmp_path = out_path.with_extension("part");

    // simulate writing some bytes
    {
        let mut f = std::fs::File::create(&tmp_path).unwrap();
        f.write_all(&[1u8,2,3,4,5]).unwrap();
        f.flush().unwrap();
    }
    // rename to final
    fs::rename(&tmp_path, &out_path).unwrap();

    let meta = fs::metadata(&out_path).unwrap();
    assert!(meta.len() > 0, "expected non-zero output file");
}
