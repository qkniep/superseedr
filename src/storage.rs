// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::errors::StorageError;
use std::path::{Path, PathBuf};
use tokio::fs::{self, try_exists, File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};

use crate::torrent_file::InfoFile;

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub path: PathBuf,            // The full path to the file on the disk.
    pub length: u64,              // The length of the file in bytes.
    pub global_start_offset: u64, // The starting offset of this file within the torrent's complete data stream.
    pub is_padding: bool,         // NEW: Indicates if this is a BEP 47 padding file.
}

/// Manages the file layout for a torrent, abstracting away the difference
/// between single and multi-file torrents.
#[derive(Debug, Clone)]
pub struct MultiFileInfo {
    pub files: Vec<FileInfo>,
    pub total_size: u64,
}

impl MultiFileInfo {
    /// Creates a new MultiFileInfo map. This is the central point of unification.
    /// It intelligently handles both single and multi-file torrent metadata.
    pub fn new(
        root_dir: &Path,
        torrent_name: &str,
        files: Option<&Vec<InfoFile>>,
        length: Option<u64>,
    ) -> std::io::Result<Self> {
        if let Some(torrent_files) = files {
            let mut files_vec = Vec::new();
            let mut current_offset = 0;

            for f in torrent_files {
                let mut full_path = root_dir.to_path_buf();
                // The path in the torrent metadata can contain subdirectories.
                for component in &f.path {
                    full_path.push(component);
                }

                // BEP 47: Check 'attr' string. If it contains 'p', it is a padding file.
                let is_padding = f.attr.as_deref().map(|s| s.contains('p')).unwrap_or(false);

                files_vec.push(FileInfo {
                    path: full_path,
                    length: f.length as u64,
                    global_start_offset: current_offset,
                    is_padding,
                });

                current_offset += f.length as u64;
            }
            Ok(Self {
                files: files_vec,
                total_size: current_offset,
            })
        } else {
            let total_size = length.unwrap_or(0);
            let file_path = root_dir.join(torrent_name);
            let single_file = FileInfo {
                path: file_path,
                length: total_size,
                global_start_offset: 0,
                is_padding: false, // Single file mode implies valid data
            };
            Ok(Self {
                files: vec![single_file],
                total_size,
            })
        }
    }
}

/// Creates all necessary directories and pre-allocates all files for a torrent.
/// This function works for both single and multi-file torrents.
pub async fn create_and_allocate_files(
    multi_file_info: &MultiFileInfo,
) -> Result<(), StorageError> {
    for file_info in &multi_file_info.files {
        if file_info.is_padding {
            continue;
        }

        // Ensure the parent directory for the file exists.
        if let Some(parent_dir) = file_info.path.parent() {
            if !try_exists(parent_dir).await? {
                fs::create_dir_all(parent_dir).await?;
            }
        }

        // Create and set the length of the file if it doesn't exist.
        if !try_exists(&file_info.path).await? {
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(&file_info.path)
                .await?;
            file.set_len(file_info.length).await?;
        }
    }
    Ok(())
}

pub async fn read_data_from_disk(
    multi_file_info: &MultiFileInfo,
    global_offset: u64,
    bytes_to_read: usize,
) -> Result<Vec<u8>, StorageError> {
    let mut buffer = Vec::with_capacity(bytes_to_read);
    let mut bytes_read = 0;

    for file_info in &multi_file_info.files {
        let file_start = file_info.global_start_offset;
        let file_end = file_start + file_info.length;
        let read_start = global_offset + bytes_read as u64;

        if read_start < file_end && global_offset < file_end {
            let local_offset = read_start.saturating_sub(file_start);
            let bytes_to_read_in_this_file = std::cmp::min(
                (bytes_to_read - bytes_read) as u64,
                file_info.length - local_offset,
            ) as usize;

            if bytes_to_read_in_this_file > 0 {
                if file_info.is_padding {
                    // This maintains offset integrity without requiring a file on disk.
                    let zeros = vec![0u8; bytes_to_read_in_this_file];
                    buffer.extend_from_slice(&zeros);
                } else {
                    let mut file = File::open(&file_info.path).await?;
                    file.seek(SeekFrom::Start(local_offset)).await?;

                    let mut temp_buf = vec![0; bytes_to_read_in_this_file];
                    file.read_exact(&mut temp_buf).await?;
                    buffer.extend_from_slice(&temp_buf);
                }

                bytes_read += bytes_to_read_in_this_file;
            }

            if bytes_read == bytes_to_read {
                return Ok(buffer);
            }
        }
    }

    Err(StorageError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "Failed to read all data, offset likely out of bounds",
    )))
}

pub async fn write_data_to_disk(
    multi_file_info: &MultiFileInfo,
    global_offset: u64,
    data_to_write: &[u8],
) -> Result<(), StorageError> {
    let mut bytes_written = 0;
    let data_len = data_to_write.len();

    for file_info in &multi_file_info.files {
        let file_start = file_info.global_start_offset;
        let file_end = file_start + file_info.length;
        let write_start = global_offset + bytes_written as u64;

        if write_start < file_end && global_offset < file_end {
            let local_offset = write_start.saturating_sub(file_start);
            let bytes_to_write_in_this_file = std::cmp::min(
                (data_len - bytes_written) as u64,
                file_info.length - local_offset,
            ) as usize;

            if bytes_to_write_in_this_file > 0 {
                if !file_info.is_padding {
                    let mut file = OpenOptions::new()
                        .write(true)
                        .create(true) // Ensure file exists if we race to write
                        .open(&file_info.path)
                        .await?;

                    file.seek(SeekFrom::Start(local_offset)).await?;

                    let data_slice =
                        &data_to_write[bytes_written..bytes_written + bytes_to_write_in_this_file];

                    file.write_all(data_slice).await?;

                    // Without this, the OS might hold the data in memory, and if the app exits immediately
                    // (like in the test case), the data is lost.
                    file.flush().await?;
                } else {
                }

                bytes_written += bytes_to_write_in_this_file;
            }

            if bytes_written == data_len {
                return Ok(());
            }
        }
    }

    tracing::error!(
        "💾 [Storage] ERROR: Write incomplete! Written: {}/{}. Global Offset: {}",
        bytes_written,
        data_len,
        global_offset
    );

    Err(StorageError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "Failed to write all data, offset likely out of bounds",
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::StorageError;
    use crate::torrent_file::InfoFile;

    use tempfile::tempdir;
    use tokio::fs::File;
    use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};

    /// Helper to create a single-file setup
    fn setup_single_file() -> (tempfile::TempDir, MultiFileInfo) {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let torrent_name = "single_file.txt";
        let length = 100;
        let mfi = MultiFileInfo::new(root, torrent_name, None, Some(length)).unwrap();
        (dir, mfi)
    }

    /// Helper to create a multi-file setup
    fn setup_multi_file() -> (tempfile::TempDir, MultiFileInfo) {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let torrent_name = "multi_file_torrent";
        let files = vec![
            InfoFile {
                path: vec!["file_a.txt".to_string()],
                length: 50, // Ends at 49
                md5sum: None,
                attr: None, // Standard file
            },
            InfoFile {
                path: vec!["subdir".to_string(), "file_b.txt".to_string()],
                length: 70, // Starts at 50
                md5sum: None,
                attr: None, // Standard file
            },
        ];
        // Total size 120
        let mfi = MultiFileInfo::new(root, torrent_name, Some(&files), None).unwrap();
        (dir, mfi)
    }

    /// Helper to create a setup with a padding file in the middle
    fn setup_padding_file_scenario() -> (tempfile::TempDir, MultiFileInfo) {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let torrent_name = "padding_test";
        // Scenario:
        // File 1: 10 bytes (Offset 0-9)
        // Padding: 5 bytes (Offset 10-14) - Should NOT be created on disk
        // File 2: 10 bytes (Offset 15-24)
        let files = vec![
            InfoFile {
                path: vec!["real_1.txt".to_string()],
                length: 10,
                md5sum: None,
                attr: None,
            },
            InfoFile {
                path: vec![".pad/10".to_string()], // Typical padding name
                length: 5,
                md5sum: None,
                attr: Some("p".to_string()), // Attribute marking padding
            },
            InfoFile {
                path: vec!["real_2.txt".to_string()],
                length: 10,
                md5sum: None,
                attr: None,
            },
        ];
        let mfi = MultiFileInfo::new(root, torrent_name, Some(&files), None).unwrap();
        (dir, mfi)
    }

    #[tokio::test]
    async fn test_multi_file_info_new_single() {
        let (dir, mfi) = setup_single_file();
        assert_eq!(mfi.files.len(), 1);
        assert_eq!(mfi.total_size, 100);
        assert_eq!(mfi.files[0].length, 100);
        assert_eq!(mfi.files[0].global_start_offset, 0);
        assert_eq!(mfi.files[0].path, dir.path().join("single_file.txt"));
        assert!(!mfi.files[0].is_padding);
    }

    #[tokio::test]
    async fn test_multi_file_info_new_multi() {
        let (dir, mfi) = setup_multi_file();
        assert_eq!(mfi.files.len(), 2);
        assert_eq!(mfi.total_size, 120);

        // File 1
        assert_eq!(mfi.files[0].length, 50);
        assert_eq!(mfi.files[0].global_start_offset, 0);
        assert_eq!(mfi.files[0].path, dir.path().join("file_a.txt"));
        assert!(!mfi.files[0].is_padding);

        // File 2
        assert_eq!(mfi.files[1].length, 70);
        assert_eq!(mfi.files[1].global_start_offset, 50);
        assert_eq!(
            mfi.files[1].path,
            dir.path().join("subdir").join("file_b.txt")
        );
        assert!(!mfi.files[1].is_padding);
    }

    #[tokio::test]
    async fn test_create_and_allocate_files_single() {
        let (_dir, mfi) = setup_single_file();
        create_and_allocate_files(&mfi).await.unwrap();

        let file_path = &mfi.files[0].path;
        assert!(tokio::fs::try_exists(file_path).await.unwrap());
        let metadata = tokio::fs::metadata(file_path).await.unwrap();
        assert_eq!(metadata.len(), 100);
    }

    #[tokio::test]
    async fn test_create_and_allocate_files_multi() {
        let (dir, mfi) = setup_multi_file();
        create_and_allocate_files(&mfi).await.unwrap();

        let file_a_path = &mfi.files[0].path;
        let file_b_path = &mfi.files[1].path;
        let subdir_path = dir.path().join("subdir");

        assert!(tokio::fs::try_exists(subdir_path).await.unwrap());
        assert!(tokio::fs::try_exists(file_a_path).await.unwrap());
        let metadata_a = tokio::fs::metadata(file_a_path).await.unwrap();
        assert_eq!(metadata_a.len(), 50);

        assert!(tokio::fs::try_exists(file_b_path).await.unwrap());
        let metadata_b = tokio::fs::metadata(file_b_path).await.unwrap();
        assert_eq!(metadata_b.len(), 70);
    }

    #[tokio::test]
    async fn test_padding_files_logic() {
        // This test verifies that padding files are correctly identified,
        // NOT created on disk, and I/O operations transparently skip them.
        let (_dir, mfi) = setup_padding_file_scenario();

        assert_eq!(mfi.files.len(), 3);
        assert!(!mfi.files[0].is_padding, "File 1 should not be padding");
        assert!(mfi.files[1].is_padding, "File 2 SHOULD be padding");
        assert!(!mfi.files[2].is_padding, "File 3 should not be padding");

        create_and_allocate_files(&mfi).await.unwrap();
        assert!(
            tokio::fs::try_exists(&mfi.files[0].path).await.unwrap(),
            "Real file 1 must exist"
        );
        assert!(
            !tokio::fs::try_exists(&mfi.files[1].path).await.unwrap(),
            "Padding file must NOT exist on disk"
        );
        assert!(
            tokio::fs::try_exists(&mfi.files[2].path).await.unwrap(),
            "Real file 2 must exist"
        );

        // We write 25 bytes starting at offset 0.
        // 0-9: Real File 1 (10 bytes)
        // 10-14: Padding (5 bytes) -> Discarded
        // 15-24: Real File 2 (10 bytes)
        let data: Vec<u8> = (0..25).collect();
        write_data_to_disk(&mfi, 0, &data).await.unwrap();

        // Read back the 25 bytes.
        // We expect: [Real Data] + [Zeros] + [Real Data]
        let read_back = read_data_from_disk(&mfi, 0, 25).await.unwrap();

        // Check first part (0-9)
        assert_eq!(read_back[0..10], data[0..10]);

        // Check padding part (10-14) - Should be Zeros, NOT the data we 'wrote'
        assert_eq!(read_back[10..15], vec![0, 0, 0, 0, 0]);

        // Check second part (15-24) - Should match original data from index 15
        assert_eq!(read_back[15..25], data[15..25]);
    }

    #[tokio::test]
    async fn test_write_read_single_file() {
        let (_dir, mfi) = setup_single_file();
        create_and_allocate_files(&mfi).await.unwrap();

        let data1: Vec<u8> = (0..20).collect(); // 20 bytes
        let data2: Vec<u8> = (20..50).collect(); // 30 bytes

        write_data_to_disk(&mfi, 10, &data1).await.unwrap();
        write_data_to_disk(&mfi, 50, &data2).await.unwrap();

        let read_data1 = read_data_from_disk(&mfi, 10, 20).await.unwrap();
        assert_eq!(data1, read_data1);

        let read_data2 = read_data_from_disk(&mfi, 50, 30).await.unwrap();
        assert_eq!(data2, read_data2);

        let empty_data = read_data_from_disk(&mfi, 0, 10).await.unwrap();
        assert_eq!(empty_data, vec![0; 10]);
    }

    #[tokio::test]
    async fn test_write_read_across_files() {
        let (_dir, mfi) = setup_multi_file(); // FileA: [0-49], FileB: [50-119]
        create_and_allocate_files(&mfi).await.unwrap();

        // Write 30 bytes starting at offset 40 (Spanning 40-69)
        let write_data: Vec<u8> = (0..30).collect();
        write_data_to_disk(&mfi, 40, &write_data).await.unwrap();

        let read_data = read_data_from_disk(&mfi, 40, 30).await.unwrap();
        assert_eq!(write_data, read_data);

        // Verify manually
        let mut file_a = File::open(&mfi.files[0].path).await.unwrap();
        file_a.seek(SeekFrom::Start(40)).await.unwrap();
        let mut buf_a = vec![0; 10];
        file_a.read_exact(&mut buf_a).await.unwrap();
        assert_eq!(buf_a, &write_data[0..10]);

        let mut file_b = File::open(&mfi.files[1].path).await.unwrap();
        let mut buf_b = vec![0; 20];
        file_b.read_exact(&mut buf_b).await.unwrap();
        assert_eq!(buf_b, &write_data[10..30]);
    }

    #[tokio::test]
    async fn test_read_out_of_bounds() {
        let (_dir, mfi) = setup_single_file(); // total_size = 100
        create_and_allocate_files(&mfi).await.unwrap();

        let res = read_data_from_disk(&mfi, 95, 10).await;
        assert!(res.is_err());
        if let Err(StorageError::Io(err)) = res {
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        } else {
            panic!("Expected Io Error");
        }

        let res_ok = read_data_from_disk(&mfi, 90, 10).await;
        assert!(res_ok.is_ok());
        assert_eq!(res_ok.unwrap().len(), 10);
    }

    #[tokio::test]
    async fn test_write_out_of_bounds() {
        let (_dir, mfi) = setup_single_file(); // total_size = 100
        create_and_allocate_files(&mfi).await.unwrap();

        let data = vec![1; 10];
        let res = write_data_to_disk(&mfi, 95, &data).await;
        assert!(res.is_err());
        if let Err(StorageError::Io(err)) = res {
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        } else {
            panic!("Expected Io Error");
        }

        let res_ok = write_data_to_disk(&mfi, 90, &data).await;
        assert!(res_ok.is_ok());

        let read_back = read_data_from_disk(&mfi, 90, 10).await.unwrap();
        assert_eq!(read_back, data);
    }
}
