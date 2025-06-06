use anyhow::{Context, Result, bail};
use std::io::Read;

pub static PROTOCAL_VER: u8 = 0;
static MAGIC: [u8; 4] = [0x20, 0x09, 0x02, 0x14];

use super::source_data::SourceDataItem;

/// Receive the mime-types and the content for the clipboard.
/// It uses a simple protocol which defines as below:
///
/// 4 bytes
/// | Item             | Bytes    | Content             |
/// |------------------| :------- | :------------------ |
/// | Magic            | 4        | 0x20 0x09 0x02 0x14 |
/// | Protocol Version | 1        | 0x00                |
/// | Section Type     | 1        | 'M'                 |
/// | Section Length   | 4        | 0x00 0x00 0x00 0x0a |
/// | Section Data     | 4        | "text/plain"        |
/// | Section Type     | 1        | 'M'                 |
/// | Section Length   | 4        | 0x00 0x00 0x00 0x04 |
/// | Section Data     | 4        | "TEXT"              |
/// | Section Type     | 1        | 'C'                 |
/// | Section Length   | 4        | 0x00 0x00 0x00 0x09 |
/// | Section Data     | 4        | "SOME Data"         |
/// | Section Type     | 1        | 'M'                 |
/// | Section Length   | 4        | 0x00 0x00 0x00 0x09 |
/// | Section Data     | 4        | "text/html"         |
/// | Section Type     | 1        | 'C'                 |
/// | Section Length   | 4        | 0x00 0x00 0x00 0x09 |
/// | Section Data     | 4        | "HTML code"         |
/// - Every section starts with the section type, `M` (mime-type) or `C` (content).
/// - Before `C` section, there must be one or more `M` section to indicate the data type.
/// - Section length will be parsed as big-endian uint32 number.
pub fn receive_data_bulk(mut reader: impl Read) -> Result<Vec<SourceDataItem>> {
    // Check magic header
    let mut magic = [0u8; 4];
    reader
        .read_exact(&mut magic)
        .context("Failed to read magic header")?;
    if magic != MAGIC {
        bail!("Failed to match magic header: {:x?}", magic);
    }

    // Check version
    let mut ver = [0u8; 1];
    reader
        .read_exact(&mut ver)
        .context("Failed to read protocal version")?;
    if ver[0] != PROTOCAL_VER {
        bail!("Failed to match protoal version: {}", ver[0]);
    }

    let mut flag = [0u8; 1];
    let mut type_list = Vec::new();
    let mut ret = Vec::<SourceDataItem>::new();
    loop {
        let r = reader.read(&mut flag).context("Failed to read flag")?;
        // EOF
        if r == 0 {
            break;
        }
        log::debug!("Read block flag '{}'", flag[0]);
        match flag[0] {
            b'M' => {
                let mime_type = read_mime_types(&mut reader)?;
                type_list.push(mime_type);
            }
            b'C' => {
                if type_list.is_empty() {
                    bail!("Failed to read content with empty mime type");
                }
                let content = read_content(&mut reader)?;
                ret.push(SourceDataItem {
                    mime_type: type_list,
                    content: content.into(),
                });
                type_list = Vec::new();
            }
            _ => {
                bail!("Failed to parse flag {}", flag[0]);
            }
        }
    }

    Ok(ret)
}

pub fn receive_data_oneshot(
    mut reader: impl Read,
    mime_types: &[String],
) -> Result<Vec<SourceDataItem>> {
    let mut content = Vec::<u8>::new();
    let n = reader
        .read_to_end(&mut content)
        .context("Failed to read content")?;
    log::debug!("Read {n} bytes for oneshot mode");

    let filtered: Vec<String> = mime_types
        .iter()
        .filter(|t| !t.is_empty())
        .map(String::clone)
        .collect();
    if filtered.is_empty() {
        bail!("All given mime_types are empty");
    }

    let ret = vec![SourceDataItem {
        mime_type: filtered,
        content: content.into(),
    }];
    Ok(ret)
}

fn read_mime_types(reader: &mut impl Read) -> Result<String> {
    let mut size_buf = [0u8; 4];
    reader
        .read_exact(&mut size_buf)
        .context("Failed to read mime type size")?;
    let size: u32 = ((size_buf[0] as u32) << 24)
        + ((size_buf[1] as u32) << 16)
        + ((size_buf[2] as u32) << 8)
        + size_buf[3] as u32;

    log::debug!("Expected mime-type size: {}", size);
    let mut buf = vec![0u8; size as usize];
    reader
        .read_exact(&mut buf)
        .context("Failed to read mime type")?;

    let mime_type = String::from_utf8(buf.to_vec())
        .with_context(|| format!("Failed to parse mime type string, {:x?}", buf))?;
    log::debug!("Received mime-type: {}", mime_type);
    Ok(mime_type)
}

fn read_content(reader: &mut impl Read) -> Result<Vec<u8>> {
    let mut size_buf = [0u8; 4];
    reader
        .read_exact(&mut size_buf)
        .context("Failed to read content size")?;
    let size: u32 = ((size_buf[0] as u32) << 24)
        + ((size_buf[1] as u32) << 16)
        + ((size_buf[2] as u32) << 8)
        + size_buf[3] as u32;

    log::debug!("Expected content size: {}", size);
    let mut buf = vec![0u8; size as usize];
    reader
        .read_exact(&mut buf)
        .context("Failed to read content")?;

    Ok(buf)
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    #[test]
    fn test_read_mime_types() {
        // Length is not 4 bytes
        let buf = [0u8, 0, 0];
        let r = read_mime_types(&mut &buf[..]);
        assert!(r.is_err());

        // Not enough data
        let buf = [0u8, 0, 0, 1];
        let r = read_mime_types(&mut &buf[..]);
        assert!(r.is_err());

        let buf = [0u8, 0, 0, 4, b't', b'e', b'x', b't'];
        let r = read_mime_types(&mut &buf[..]).unwrap();
        assert_eq!(r, "text");
    }

    #[test]
    fn test_read_content() {
        // Length is not 4 bytes
        let buf = [0u8, 0, 0];
        let r = read_content(&mut &buf[..]);
        assert!(r.is_err());

        // Not enough data
        let buf = [0u8, 0, 0, 1];
        let r = read_content(&mut &buf[..]);
        assert!(r.is_err());

        let buf = [0u8, 0, 0, 5, b't', b'e', b'x', b't', 0x42];
        let r = read_content(&mut &buf[..]).unwrap();
        assert_eq!(r, vec![b't', b'e', b'x', b't', 0x42]);

        // Read 256+ bytes
        let mut buf = [0u8; (1 << 8) + 4];
        buf[2] = 1;
        let r = read_content(&mut &buf[..]).unwrap();
        assert_eq!(r, [0u8; 1 << 8]);
    }

    #[test]
    fn test_receive_data_bulk() {
        // Wrong magic
        let buf = [0x02, 0x09, 0x02, 0x14, PROTOCAL_VER, b'M'];
        let r = receive_data_bulk(&mut &buf[..]);
        assert!(r.is_err());

        // Wrong protoal version
        let buf = [0x20, 0x09, 0x02, 0x14, 99, b'M'];
        let r = receive_data_bulk(&mut &buf[..]);
        assert!(r.is_err());

        // correct
        #[rustfmt::skip]
        let buf =
            [0x20, 0x09, 0x02, 0x14, PROTOCAL_VER,
            b'M', 0, 0, 0, 10, b't', b'e', b'x', b't', b'/', b'p', b'l', b'a', b'i', b'n',
            b'M', 0, 0, 0, 4, b'T', b'E', b'X', b'T',
            b'C', 0, 0, 0, 4, b'G', b'O', b'O', b'D',
            b'M', 0, 0, 0, 9, b't', b'e', b'x', b't', b'/', b'h', b't', b'm', b'l',
            b'C', 0, 0, 0, 3, b'B', b'A', b'D',
            ];
        let r = receive_data_bulk(&mut &buf[..]).unwrap();
        assert_eq!(r.len(), 2);

        let data1 = &r[0];
        assert_eq!(data1.mime_type.len(), 2);
        assert_eq!(data1.mime_type[0], "text/plain");
        assert_eq!(data1.mime_type[1], "TEXT");
        assert_eq!(data1.content.as_slice(), b"GOOD");
        let data2 = &r[1];
        assert_eq!(data2.mime_type.len(), 1);
        assert_eq!(data2.mime_type[0], "text/html");
        assert_eq!(data2.content.as_slice(), b"BAD");
    }

    #[test]
    fn test_receive_data_oneshot() {
        let buf = [b'G', b'O', b'O', b'D'];

        // With one mime-type
        let r = receive_data_oneshot(&mut &buf[..], &["text".to_string()]).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].mime_type.len(), 1);
        assert_eq!(r[0].mime_type[0], "text");

        // With two mime-types
        let r = receive_data_oneshot(
            &mut &buf[..],
            &["text".to_string(), "text/plain".to_string()],
        )
        .unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].mime_type.len(), 2);
        assert_eq!(r[0].mime_type[0], "text");
        assert_eq!(r[0].mime_type[1], "text/plain");

        // With zero mime-type
        let r = receive_data_oneshot(&mut &buf[..], &["".to_string()]);
        assert!(r.is_err())
    }
}
