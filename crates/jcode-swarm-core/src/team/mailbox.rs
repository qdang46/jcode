//! File-based mailbox — port of `team-mailbox/{send,inbox,poll,ack}.ts`.
//!
//! Messages are JSON files under `runtime/{run}/inboxes/{member}/`. Writes are
//! atomic and guarded by a per-inbox lock; acked messages move to `processed/`.

use std::fs;
use std::path::Path;

use crate::team::locks::{atomic_write, read_json, with_lock};
use crate::team::paths::inbox_dir;
use crate::team::spec::*;
use crate::team::state::load_runtime;

/// Caller context for a send (mirrors the TS `SendContext`).
pub struct SendContext<'a> {
    pub is_lead: bool,
    pub active_members: &'a [String],
    pub reserved_recipients: &'a [String],
    pub recipient_unread_max_bytes: usize,
}

impl<'a> SendContext<'a> {
    /// Convenience: a lead context with default backpressure ceiling.
    pub fn lead(active_members: &'a [String]) -> Self {
        Self {
            is_lead: true,
            active_members,
            reserved_recipients: &[],
            recipient_unread_max_bytes: TEAM_RECIPIENT_UNREAD_MAX_BYTES,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SendResult {
    pub message_id: String,
    pub delivered_to: Vec<String>,
}

/// Send a message to one recipient (or broadcast with `to = "*"`, lead only).
/// Validation order matches the reference exactly.
pub fn send_message(msg: &TeamMessage, run_id: &str, ctx: &SendContext) -> TeamResult<SendResult> {
    let serialized = format!("{}\n", serde_json::to_string_pretty(msg)?);
    let serialized_bytes = serialized.len();

    if msg.body.len() > TEAM_MESSAGE_MAX_BYTES {
        return Err(TeamError::PayloadTooLarge);
    }

    // assertTeamAcceptsMessages: a missing state file is tolerated.
    match load_runtime(run_id) {
        Ok(state) => {
            if matches!(
                state.status,
                RuntimeStatus::Deleting | RuntimeStatus::Deleted
            ) {
                return Err(TeamError::TeamDeleting);
            }
        }
        Err(TeamError::NotFound(_)) => {}
        Err(e) => return Err(e),
    }

    if msg.to == "*" && !ctx.is_lead {
        return Err(TeamError::BroadcastNotPermitted);
    }

    let recipients: Vec<String> = if msg.to == "*" {
        let mut v = ctx.active_members.to_vec();
        v.sort();
        v.dedup();
        v
    } else {
        vec![msg.to.clone()]
    };

    let mut delivered = Vec::new();
    for recipient in recipients {
        let inbox = inbox_dir(run_id, &recipient);
        fs::create_dir_all(&inbox)?;
        let lock = inbox.join(".lock");
        let recipient_for_closure = recipient.clone();
        with_lock(&lock, &format!("team-mailbox:{recipient}"), || {
            let unread = unread_size_bytes(&inbox)?;
            if unread + serialized_bytes > ctx.recipient_unread_max_bytes {
                return Err(TeamError::RecipientBackpressure);
            }
            let unreserved = inbox.join(format!("{}.json", msg.message_id));
            let reserved = inbox.join(format!(".delivering-{}.json", msg.message_id));
            if unreserved.exists() || reserved.exists() {
                return Err(TeamError::DuplicateMessageId(msg.message_id.clone()));
            }
            let target = if ctx.reserved_recipients.contains(&recipient_for_closure) {
                reserved
            } else {
                unreserved
            };
            atomic_write(&target, &serialized)?;
            Ok(())
        })?;
        delivered.push(recipient);
    }

    Ok(SendResult {
        message_id: msg.message_id.clone(),
        delivered_to: delivered,
    })
}

/// Sum sizes of unread message files (`*.json` and `.delivering-*.json`).
/// The `processed/` subdir is a directory, so the `is_file` check skips it.
fn unread_size_bytes(inbox: &Path) -> TeamResult<usize> {
    let mut total = 0usize;
    let rd = match fs::read_dir(inbox) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(TeamError::Io(e)),
    };
    for entry in rd.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".json") {
            continue;
        }
        let is_delivering = name.starts_with(".delivering-");
        // skip the inbox `.lock` and other dotfiles that aren't deliveries
        if name.starts_with('.') && !is_delivering {
            continue;
        }
        total += entry.metadata().map(|m| m.len() as usize).unwrap_or(0);
    }
    Ok(total)
}

/// List unread messages, skipping malformed files, sorted ascending by timestamp.
pub fn list_unread(run_id: &str, member: &str) -> TeamResult<Vec<TeamMessage>> {
    let inbox = inbox_dir(run_id, member);
    let mut out = Vec::new();
    let rd = match fs::read_dir(&inbox) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(TeamError::Io(e)),
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !name.ends_with(".json") {
            continue;
        }
        if let Ok(m) = read_json::<TeamMessage>(&entry.path()) {
            out.push(m); // skip malformed/unreadable, like the reference
        }
    }
    out.sort_by_key(|m| m.timestamp);
    Ok(out)
}

/// Return unread messages whose ids are not in `already_pending`
/// (port of poll.ts injection filtering).
pub fn poll_messages(
    run_id: &str,
    member: &str,
    already_pending: &[String],
) -> TeamResult<Vec<TeamMessage>> {
    let unread = list_unread(run_id, member)?;
    Ok(unread
        .into_iter()
        .filter(|m| !already_pending.contains(&m.message_id))
        .collect())
}

/// Move acked messages into `processed/` (port of ack.ts).
pub fn acknowledge(run_id: &str, member: &str, message_ids: &[String]) -> TeamResult<()> {
    let inbox = inbox_dir(run_id, member);
    let processed = inbox.join("processed");
    fs::create_dir_all(&processed)?;
    for id in message_ids {
        let target = processed.join(format!("{id}.json"));
        for src in [
            inbox.join(format!("{id}.json")),
            inbox.join(format!(".delivering-{id}.json")),
        ] {
            match fs::rename(&src, &target) {
                Ok(()) => break,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(TeamError::Io(e)),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: &str, to: &str, body: &str) -> TeamMessage {
        TeamMessage {
            version: 1,
            message_id: id.into(),
            from: "lead".into(),
            to: to.into(),
            kind: MessageKind::Message,
            body: body.into(),
            summary: None,
            references: vec![],
            timestamp: 1,
            correlation_id: None,
            color: None,
        }
    }

    #[test]
    fn send_then_list_then_ack() {
        let base = crate::team::test_support::guarded_base();
        let run = base.run_id();
        let members = vec!["worker".to_string()];
        send_message(
            &msg("m1", "worker", "hello"),
            &run,
            &SendContext::lead(&members),
        )
        .unwrap();
        let unread = list_unread(&run, "worker").unwrap();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].body, "hello");
        acknowledge(&run, "worker", &["m1".into()]).unwrap();
        assert!(list_unread(&run, "worker").unwrap().is_empty());
    }

    #[test]
    fn duplicate_message_id_rejected() {
        let base = crate::team::test_support::guarded_base();
        let run = base.run_id();
        let members = vec!["worker".to_string()];
        send_message(
            &msg("dup", "worker", "a"),
            &run,
            &SendContext::lead(&members),
        )
        .unwrap();
        let err = send_message(
            &msg("dup", "worker", "b"),
            &run,
            &SendContext::lead(&members),
        )
        .unwrap_err();
        assert!(matches!(err, TeamError::DuplicateMessageId(_)));
    }

    #[test]
    fn broadcast_requires_lead() {
        let base = crate::team::test_support::guarded_base();
        let run = base.run_id();
        let members = vec!["a".to_string(), "b".to_string()];
        let ctx = SendContext {
            is_lead: false,
            active_members: &members,
            reserved_recipients: &[],
            recipient_unread_max_bytes: TEAM_RECIPIENT_UNREAD_MAX_BYTES,
        };
        let err = send_message(&msg("b1", "*", "hi"), &run, &ctx).unwrap_err();
        assert!(matches!(err, TeamError::BroadcastNotPermitted));
    }

    #[test]
    fn lead_broadcast_delivers_to_all_active() {
        let base = crate::team::test_support::guarded_base();
        let run = base.run_id();
        let members = vec!["a".to_string(), "b".to_string()];
        let res = send_message(&msg("bc", "*", "all"), &run, &SendContext::lead(&members)).unwrap();
        assert_eq!(res.delivered_to.len(), 2);
        assert_eq!(list_unread(&run, "a").unwrap().len(), 1);
        assert_eq!(list_unread(&run, "b").unwrap().len(), 1);
    }

    #[test]
    fn payload_too_large_rejected() {
        let base = crate::team::test_support::guarded_base();
        let run = base.run_id();
        let members = vec!["w".to_string()];
        let big = "x".repeat(TEAM_MESSAGE_MAX_BYTES + 1);
        let err =
            send_message(&msg("p", "w", &big), &run, &SendContext::lead(&members)).unwrap_err();
        assert!(matches!(err, TeamError::PayloadTooLarge));
    }

    #[test]
    fn backpressure_blocks_when_inbox_full() {
        let base = crate::team::test_support::guarded_base();
        let run = base.run_id();
        let members = vec!["w".to_string()];
        // First message lands with a generous ceiling.
        send_message(&msg("a", "w", "hello"), &run, &SendContext::lead(&members)).unwrap();
        // With unread > 0 and a tiny ceiling, the next send hits backpressure.
        let tiny = SendContext {
            is_lead: true,
            active_members: &members,
            reserved_recipients: &[],
            recipient_unread_max_bytes: 10,
        };
        let err = send_message(&msg("b", "w", "hello"), &run, &tiny).unwrap_err();
        assert!(matches!(err, TeamError::RecipientBackpressure));
    }

    #[test]
    fn list_unread_sorted_by_timestamp() {
        let base = crate::team::test_support::guarded_base();
        let run = base.run_id();
        let members = vec!["w".to_string()];
        let mut late = msg("late", "w", "2");
        late.timestamp = 200;
        let mut early = msg("early", "w", "1");
        early.timestamp = 100;
        send_message(&late, &run, &SendContext::lead(&members)).unwrap();
        send_message(&early, &run, &SendContext::lead(&members)).unwrap();
        let unread = list_unread(&run, "w").unwrap();
        assert_eq!(unread[0].message_id, "early");
        assert_eq!(unread[1].message_id, "late");
    }

    #[test]
    fn malformed_message_file_skipped() {
        let base = crate::team::test_support::guarded_base();
        let run = base.run_id();
        let members = vec!["w".to_string()];
        send_message(&msg("ok", "w", "good"), &run, &SendContext::lead(&members)).unwrap();
        fs::write(inbox_dir(&run, "w").join("junk.json"), b"{not json").unwrap();
        let unread = list_unread(&run, "w").unwrap();
        assert_eq!(
            unread.len(),
            1,
            "malformed file skipped, valid one survives"
        );
    }

    #[test]
    fn poll_filters_already_pending() {
        let base = crate::team::test_support::guarded_base();
        let run = base.run_id();
        let members = vec!["w".to_string()];
        send_message(&msg("m1", "w", "a"), &run, &SendContext::lead(&members)).unwrap();
        send_message(&msg("m2", "w", "b"), &run, &SendContext::lead(&members)).unwrap();
        let fresh = poll_messages(&run, "w", &["m1".to_string()]).unwrap();
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].message_id, "m2");
    }
}
