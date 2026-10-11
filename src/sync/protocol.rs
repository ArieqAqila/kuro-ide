//! Messages between terminals (clients) and the leader, and the log entry type shared by both.

use super::ot::Op;
use serde::{Deserialize, Serialize};

pub type ClientId = u64;

/// Identifies one client message; unique per client. Lets the leader recognise an edit it already committed
/// (for example when a client re-sends after a failover) instead of applying it twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MsgId {
    pub client: ClientId,
    pub seq: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Edit,
    Undo,
    Redo,
    /// The file changed on disk and was reloaded into the shared buffer.
    External,
}

/// One entry of the leader's ordered history. `rev` counts committed entries; entries are applied in `rev` order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Committed {
    pub rev: u64,
    pub origin: ClientId,
    pub id: Option<MsgId>,
    pub kind: Kind,
    pub ops: Vec<Op>,
    /// Where the requester's cursor should go (undo/redo): char position after the change.
    pub focus: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientMsg {
    Hello {
        client: ClientId,
    },
    /// After a leader change: everything this client knows, so the new leader can continue seamlessly.
    Rejoin {
        client: ClientId,
        epoch: u64,
        rev: u64,
        recent: Vec<Committed>,
        in_flight: Option<InFlight>,
        saved_rev: u64,
        disk_hash: String,
    },
    Edit {
        id: MsgId,
        base_rev: u64,
        ops: Vec<Op>,
    },
    Undo {
        id: MsgId,
    },
    Redo {
        id: MsgId,
    },
    Save {
        id: MsgId,
        force: bool,
    },
    Bye {
        client: ClientId,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InFlight {
    pub id: MsgId,
    /// Revision the ops are expressed against (the client's current confirmed revision).
    pub base_rev: u64,
    pub ops: Vec<Op>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServerMsg {
    /// First message to a joining client: the whole shared text.
    Welcome {
        epoch: u64,
        leader: ClientId,
        rev: u64,
        text: String,
        saved_rev: u64,
        disk_hash: String,
        conflict: bool,
        clients: Vec<ClientId>,
    },
    /// Reply to a `Rejoin`: from here on the stream continues with `Apply`/`Ack` entries after the client's revision.
    Rejoined {
        epoch: u64,
        leader: ClientId,
        rev: u64,
        saved_rev: u64,
        conflict: bool,
    },
    /// Your `Edit` was committed at `rev`.
    Ack {
        id: MsgId,
        rev: u64,
    },
    /// An entry to apply to your text.
    Apply(Committed),
    Saved {
        rev: u64,
        disk_hash: String,
    },
    SaveRefused {
        id: MsgId,
        reason: String,
    },
    SaveFailed {
        id: MsgId,
        reason: String,
    },
    /// The file changed on disk while there are unsaved edits: saving needs `force`.
    Conflict {
        reason: String,
    },
    /// The file changed on disk and was reloaded (no unsaved edits).
    Reloaded {
        rev: u64,
        disk_hash: String,
    },
    Notice {
        text: String,
    },
    Roster {
        clients: Vec<ClientId>,
    },
}

/// Encode a message as one line of JSON (the socket framing is "one JSON message per line").
pub fn encode<T: Serialize>(m: &T) -> String {
    serde_json::to_string(m).expect("messages serialise")
}

pub fn decode<'a, T: Deserialize<'a>>(line: &'a str) -> Result<T, String> {
    serde_json::from_str(line).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::super::ot::{del, ins};
    use super::*;

    fn rt<T: Serialize + for<'a> Deserialize<'a> + PartialEq + std::fmt::Debug>(m: T) {
        let s = encode(&m);
        assert!(!s.contains('\n'), "one message per line");
        assert_eq!(decode::<T>(&s).unwrap(), m);
    }

    #[test]
    fn every_message_round_trips_through_json_on_one_line() {
        let id = MsgId { client: 7, seq: 3 };
        let c = Committed {
            rev: 5,
            origin: 7,
            id: Some(id),
            kind: Kind::Edit,
            ops: vec![ins(0, "a\nb"), del(1, "é😀"), Op::Mark],
            focus: Some(2),
        };
        rt(ClientMsg::Hello { client: 1 });
        rt(ClientMsg::Edit { id, base_rev: 4, ops: vec![ins(0, "x\ny")] });
        rt(ClientMsg::Undo { id });
        rt(ClientMsg::Redo { id });
        rt(ClientMsg::Save { id, force: true });
        rt(ClientMsg::Bye { client: 1 });
        rt(ClientMsg::Rejoin {
            client: 2,
            epoch: 1,
            rev: 4,
            recent: vec![c.clone()],
            in_flight: Some(InFlight { id, base_rev: 4, ops: vec![ins(1, "q")] }),
            saved_rev: 2,
            disk_hash: "abc".into(),
        });
        rt(ServerMsg::Welcome {
            epoch: 1,
            leader: 1,
            rev: 9,
            text: "hello\n日本".into(),
            saved_rev: 9,
            disk_hash: "h".into(),
            conflict: false,
            clients: vec![1, 2],
        });
        rt(ServerMsg::Rejoined { epoch: 2, leader: 2, rev: 9, saved_rev: 9, conflict: true });
        rt(ServerMsg::Ack { id, rev: 6 });
        rt(ServerMsg::Apply(c));
        rt(ServerMsg::Saved { rev: 6, disk_hash: "h".into() });
        rt(ServerMsg::SaveRefused { id, reason: "r".into() });
        rt(ServerMsg::SaveFailed { id, reason: "r".into() });
        rt(ServerMsg::Conflict { reason: "r".into() });
        rt(ServerMsg::Reloaded { rev: 1, disk_hash: "h".into() });
        rt(ServerMsg::Notice { text: "n".into() });
        rt(ServerMsg::Roster { clients: vec![] });
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        for bad in ["", "{", "[]", "{\"Nope\":1}", "null", "\u{0}"] {
            assert!(decode::<ClientMsg>(bad).is_err(), "{bad:?}");
            assert!(decode::<ServerMsg>(bad).is_err(), "{bad:?}");
        }
    }
}
