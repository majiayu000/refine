use super::{load_one_session, read_session_summaries, strings, ProcessRunner, Runner};
use anyhow::{bail, ensure, Context, Result};

pub fn load_document_content(session_ref: &str, expected_hash: &str) -> Result<String> {
    load_document_content_with_runner(&ProcessRunner, session_ref, expected_hash)
}

pub(super) fn load_document_content_with_runner<R: Runner>(
    runner: &R,
    session_ref: &str,
    expected_hash: &str,
) -> Result<String> {
    let (host, source_root, project, session_id) = decode_session_ref(session_ref)?;
    let args = strings(&[
        "raw",
        "sessions",
        "--project",
        &project,
        "--sample",
        "0",
        "--json",
    ]);
    let summaries = read_session_summaries(runner, &args)?;
    let summary = summaries
        .into_iter()
        .find(|summary| {
            summary.session_ref == session_ref
                && summary.host == host
                && summary.source_root == source_root
                && summary.project == project
                && summary.session_id == session_id
        })
        .context("Remem raw-session v2 reference no longer resolves to its exact selector")?;
    ensure!(
        summary.content_hash == expected_hash,
        "stored Remem snapshot hash drifted from the current session summary"
    );

    let loaded = load_one_session(runner, summary)?;
    Ok(loaded.session.to_document_content())
}

pub(super) fn decode_session_ref(value: &str) -> Result<(String, String, String, String)> {
    let encoded = value
        .strip_prefix("remem://raw-session/v2/")
        .context("document is not a Remem raw-session v2 reference")?;
    let parts = encoded.split('/').collect::<Vec<_>>();
    if parts.len() != 4 {
        bail!("Remem session reference has an invalid selector shape");
    }
    Ok((
        decode_hex(parts[0])?,
        decode_hex(parts[1])?,
        decode_hex(parts[2])?,
        decode_hex(parts[3])?,
    ))
}

fn decode_hex(value: &str) -> Result<String> {
    if value.is_empty() || !value.len().is_multiple_of(2) {
        bail!("Remem session reference has invalid hex encoding");
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    let encoded = value.as_bytes();
    for pair_start in (0..encoded.len()).step_by(2) {
        let pair = &encoded[pair_start..pair_start + 2];
        let text = std::str::from_utf8(pair).context("session reference hex is not UTF-8")?;
        bytes.push(u8::from_str_radix(text, 16).context("session reference hex is invalid")?);
    }
    String::from_utf8(bytes).context("session reference component is not UTF-8")
}

#[cfg(test)]
mod tests {
    use super::decode_session_ref;

    #[test]
    fn decodes_exact_v2_selector() {
        let decoded = decode_session_ref(
            "remem://raw-session/v2/636f6465782d636c69/6c6f63616c/2f7265706f/7331",
        )
        .unwrap();
        assert_eq!(
            decoded,
            (
                "codex-cli".into(),
                "local".into(),
                "/repo".into(),
                "s1".into()
            )
        );
    }
}
