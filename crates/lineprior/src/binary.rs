use crate::{Error, PriorAction, PriorBook, PriorEntry, Result};
use std::io::{Read, Write};

const MAGIC: &[u8; 4] = b"LPB\0";
const FORMAT_VERSION: u16 = 1;
const MAX_ITEMS: u32 = 10_000_000;
const MAX_BYTES: u32 = 16 * 1024 * 1024;

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidBinary {
        message: message.into(),
    }
}
fn put_string(mut w: impl Write, s: &str) -> Result<()> {
    let bytes = s.as_bytes();
    let len = u32::try_from(bytes.len()).map_err(|_| invalid("string is too large"))?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(bytes)?;
    Ok(())
}
fn get_string(mut r: impl Read) -> Result<String> {
    let mut b = [0; 4];
    r.read_exact(&mut b)?;
    let len = u32::from_le_bytes(b);
    if len > MAX_BYTES {
        return Err(invalid("string exceeds 16 MiB limit"));
    }
    let mut bytes = vec![0; len as usize];
    r.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|_| invalid("string is not valid UTF-8"))
}
fn get_optional_f64(mut r: impl Read) -> Result<Option<f64>> {
    let mut flag = [0];
    r.read_exact(&mut flag)?;
    match flag[0] {
        0 => Ok(None),
        1 => {
            let mut b = [0; 8];
            r.read_exact(&mut b)?;
            Ok(Some(f64::from_le_bytes(b)))
        }
        _ => Err(invalid("invalid optional-value flag")),
    }
}
fn put_action(mut w: impl Write, a: &PriorAction) -> Result<()> {
    put_string(&mut w, &a.action)?;
    w.write_all(&a.count.to_le_bytes())?;
    w.write_all(&a.weighted_count.to_le_bytes())?;
    for value in [a.success_rate, a.mean_score] {
        w.write_all(&[value.is_some() as u8])?;
        if let Some(v) = value {
            w.write_all(&v.to_le_bytes())?;
        }
    }
    w.write_all(&a.prior.to_le_bytes())?;
    w.write_all(&a.confidence.to_le_bytes())?;
    Ok(())
}
fn get_action(mut r: impl Read) -> Result<PriorAction> {
    let action = get_string(&mut r)?;
    let mut b8 = [0; 8];
    r.read_exact(&mut b8)?;
    let count = u64::from_le_bytes(b8);
    r.read_exact(&mut b8)?;
    let weighted_count = f64::from_le_bytes(b8);
    let success_rate = get_optional_f64(&mut r)?;
    let mean_score = get_optional_f64(&mut r)?;
    r.read_exact(&mut b8)?;
    let prior = f64::from_le_bytes(b8);
    r.read_exact(&mut b8)?;
    let confidence = f64::from_le_bytes(b8);
    Ok(PriorAction {
        action,
        count,
        weighted_count,
        success_rate,
        mean_score,
        prior,
        confidence,
    })
}

/// Writes deterministic compact binary format LPB v1, including context entries.
pub fn save_prior_book_binary(book: &PriorBook, mut writer: impl Write) -> Result<()> {
    writer.write_all(MAGIC)?;
    writer.write_all(&FORMAT_VERSION.to_le_bytes())?;
    writer.write_all(&0u16.to_le_bytes())?;
    let all: Vec<PriorEntry> = book
        .entries_sorted()
        .into_iter()
        .chain(book.context_entries_sorted())
        .collect();
    let n = u32::try_from(all.len()).map_err(|_| invalid("too many entries"))?;
    writer.write_all(&n.to_le_bytes())?;
    for entry in all {
        put_string(&mut writer, &entry.state)?;
        let c =
            u32::try_from(entry.context.len()).map_err(|_| invalid("too many context actions"))?;
        writer.write_all(&c.to_le_bytes())?;
        for s in entry.context {
            put_string(&mut writer, &s)?;
        }
        let a = u32::try_from(entry.actions.len()).map_err(|_| invalid("too many actions"))?;
        writer.write_all(&a.to_le_bytes())?;
        for action in entry.actions {
            put_action(&mut writer, &action)?;
        }
    }
    writer.flush()?;
    Ok(())
}

/// Reads LPB v1 with allocation caps; trailing bytes are rejected for corruption detection.
pub fn load_prior_book_binary(mut reader: impl Read) -> Result<PriorBook> {
    let mut magic = [0; 4];
    reader.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(invalid("bad magic"));
    }
    let mut b2 = [0; 2];
    reader.read_exact(&mut b2)?;
    if u16::from_le_bytes(b2) != FORMAT_VERSION {
        return Err(invalid("unsupported format version"));
    }
    reader.read_exact(&mut b2)?;
    let mut b4 = [0; 4];
    reader.read_exact(&mut b4)?;
    let n = u32::from_le_bytes(b4);
    if n > MAX_ITEMS {
        return Err(invalid("too many entries"));
    }
    let mut book = PriorBook::default();
    for _ in 0..n {
        let state = get_string(&mut reader)?;
        reader.read_exact(&mut b4)?;
        let c = u32::from_le_bytes(b4);
        if c > MAX_ITEMS {
            return Err(invalid("too much context"));
        }
        let mut context = Vec::with_capacity(c as usize);
        for _ in 0..c {
            context.push(get_string(&mut reader)?);
        }
        reader.read_exact(&mut b4)?;
        let a = u32::from_le_bytes(b4);
        if a > MAX_ITEMS {
            return Err(invalid("too many actions"));
        }
        let mut actions = Vec::with_capacity(a as usize);
        for _ in 0..a {
            actions.push(get_action(&mut reader)?);
        }
        if context.is_empty() {
            book.entries.insert(state, actions);
        } else {
            book.context_entries.insert((context, state), actions);
        }
    }
    let mut extra = [0; 1];
    if reader.read(&mut extra)? != 0 {
        return Err(invalid("trailing bytes"));
    }
    Ok(book)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load_prior_book;

    #[test]
    fn binary_round_trip_is_lossless_and_deterministic() {
        let json = concat!(
            r#"{"state":"s","actions":[{"action":"a","count":2,"weighted_count":2.0,"success_rate":1.0,"mean_score":null,"prior":0.7,"confidence":0.1}]}"#,
            "\n",
            r#"{"state":"s","context":["x"],"actions":[{"action":"b","count":1,"weighted_count":1.0,"success_rate":null,"mean_score":0.2,"prior":1.0,"confidence":0.05}]}"#,
            "\n"
        );
        let book = load_prior_book(json.as_bytes()).unwrap();
        let mut first = Vec::new();
        let mut second = Vec::new();
        save_prior_book_binary(&book, &mut first).unwrap();
        save_prior_book_binary(&book, &mut second).unwrap();
        assert_eq!(first, second);
        let round_trip = load_prior_book_binary(first.as_slice()).unwrap();
        assert_eq!(round_trip.entries, book.entries);
        assert_eq!(round_trip.context_entries, book.context_entries);
    }

    #[test]
    fn binary_rejects_trailing_bytes() {
        let mut bytes = Vec::new();
        save_prior_book_binary(&PriorBook::default(), &mut bytes).unwrap();
        bytes.push(1);
        assert!(matches!(
            load_prior_book_binary(bytes.as_slice()),
            Err(Error::InvalidBinary { .. })
        ));
    }
}
