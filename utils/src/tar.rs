use sha2::{Digest, Sha256};
use std::io::Read;

pub fn zip_bytes_to_targz(bytes: &[u8]) -> Vec<u8> {
    // Convert ZIP bytes to a valid tar.gz: unpack ZIP, tar entries, then gzip
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::{Cursor, Read, Write};
    use tar::{Builder, Header};
    use zip::ZipArchive;

    // Read ZIP archive from memory
    let reader = Cursor::new(bytes);
    let mut zip = ZipArchive::new(reader).expect("Failed to read ZIP archive");
    // Prepare buffer for tar data
    let mut tar_buf = Vec::new();
    {
        let mut tar = Builder::new(&mut tar_buf);
        for i in 0..zip.len() {
            let mut file = zip.by_index(i).expect("Failed to access ZIP entry");
            let mut data = Vec::new();
            file.read_to_end(&mut data)
                .expect("Failed to read ZIP entry");
            let mut header = Header::new_gnu();
            header.set_path(file.name()).expect("Invalid path");
            header.set_size(data.len() as u64);
            header.set_cksum();
            tar.append(&header, &data[..])
                .expect("Failed to append to tar");
        }
        tar.finish().expect("Failed to finish tar archive");
    }
    // Gzip compress the tar data
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&tar_buf)
        .expect("Failed to write to GzEncoder");
    encoder.finish().expect("Failed to finish GzEncoder")
}

pub fn targz_to_zip_bytes(targz: &[u8]) -> Vec<u8> {
    // Decompress the .tar.gz, then repackage the tar entries into a .zip archive
    use flate2::read::GzDecoder;
    use std::io::{Cursor, Read, Write};
    use tar::Archive;
    use zip::write::{FileOptions, ZipWriter};
    use zip::CompressionMethod;

    // Decode gzip into raw tar data
    let mut decoder = GzDecoder::new(targz);
    let mut tar_data = Vec::new();
    decoder
        .read_to_end(&mut tar_data)
        .expect("Failed to decompress tar.gz");
    // Read tar entries
    let mut archive = Archive::new(Cursor::new(tar_data));
    // Prepare zip writer
    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = FileOptions::default().compression_method(CompressionMethod::Deflated);
    for entry in archive.entries().expect("Failed to read tar entries") {
        let mut entry = entry.expect("Failed to access tar entry");
        let path = entry.path().expect("Invalid entry path").into_owned();
        let mut data = Vec::new();
        entry
            .read_to_end(&mut data)
            .expect("Failed to read tar entry");
        // Write file entry into zip
        zip.start_file(path.to_string_lossy(), options)
            .expect("Failed to start zip entry");
        zip.write_all(&data).expect("Failed to write zip entry");
    }
    // Finalize zip archive and return bytes
    let cursor = zip.finish().expect("Failed to finalize zip");
    cursor.into_inner()
}

pub fn get_diff_id_from_zip(zip_bytes: &[u8]) -> Result<String, anyhow::Error> {
    let diff_id = format!("sha256:{:x}", Sha256::digest(zip_bytes));
    Ok(diff_id)
}
