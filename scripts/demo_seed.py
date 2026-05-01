#!/usr/bin/env python3
"""Seed a local sshoosh SQLite database with six months of demo data."""

from __future__ import annotations

import argparse
import json
import sqlite3
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MIGRATIONS = [
    ("20260430000000_initial", ROOT / "migrations/20260430000000_initial.sql"),
    ("20260430000001_pending_username", ROOT / "migrations/20260430000001_pending_username.sql"),
    ("20260430000001_remote_security", ROOT / "migrations/20260430000001_remote_security.sql"),
]


@dataclass(frozen=True)
class Account:
    id: str
    username: str
    display_name: str


@dataclass(frozen=True)
class DemoPerson:
    id: str
    username: str
    display_name: str
    role: str


@dataclass(frozen=True)
class Channel:
    id: str
    slug: str
    name: str
    visibility: str
    topic: str
    members: tuple[str, ...]


DEMO_PEOPLE = [
    DemoPerson("demo-account-marco", "marco", "Marco Bellini", "admin"),
    DemoPerson("demo-account-nina", "nina", "Nina Patel", "member"),
    DemoPerson("demo-account-sam", "sam", "Sam Rivera", "member"),
    DemoPerson("demo-account-priya", "priya", "Priya Shah", "member"),
    DemoPerson("demo-account-jules", "jules", "Jules Martin", "member"),
    DemoPerson("demo-account-zoe", "zoe", "Zoe Kim", "member"),
    DemoPerson("demo-account-omar", "omar", "Omar Haddad", "member"),
    DemoPerson("demo-account-ivy", "ivy", "Ivy Chen", "member"),
    DemoPerson("demo-account-luca", "luca", "Luca Rossi", "member"),
    DemoPerson("demo-account-maya", "maya", "Maya Johnson", "member"),
]

CHANNELS = [
    Channel(
        "demo-channel-general",
        "general",
        "general",
        "public",
        "Company-wide updates and default coordination",
        ("anchor", "marco", "nina", "sam", "priya", "jules", "zoe", "omar", "ivy", "luca", "maya"),
    ),
    Channel(
        "demo-channel-engineering",
        "engineering",
        "engineering",
        "public",
        "Backend, infra, search, and deploy work",
        ("anchor", "marco", "sam", "priya", "omar", "luca"),
    ),
    Channel(
        "demo-channel-product",
        "product",
        "product",
        "public",
        "Planning, scoping, and weekly product review",
        ("anchor", "marco", "nina", "jules", "maya"),
    ),
    Channel(
        "demo-channel-design",
        "design",
        "design",
        "public",
        "UI direction, copy, and onboarding polish",
        ("anchor", "nina", "jules", "zoe", "maya"),
    ),
    Channel(
        "demo-channel-support",
        "support",
        "support",
        "public",
        "Customer issues, triage, and incident follow-up",
        ("anchor", "sam", "ivy", "luca", "maya"),
    ),
    Channel(
        "demo-channel-launch",
        "launch-room",
        "launch-room",
        "private",
        "Private launch coordination and rollout tracking",
        ("anchor", "marco", "nina", "sam", "priya", "zoe"),
    ),
    Channel(
        "demo-channel-leadership",
        "leadership",
        "leadership",
        "private",
        "Hiring, budget, and staffing decisions",
        ("anchor", "marco", "maya"),
    ),
]

THREAD_TOPICS = [
    ("Release checklist", "Need final sign-off on rollout, backup verification, and support copy."),
    ("Search quality pass", "Recent indexing changes improved recall, but ranking still needs trimming."),
    ("Onboarding friction", "The first session still asks for too much context too early."),
    ("Tunnel stability", "A few reconnects looked harmless, but users lost draft text."),
    ("Docs cleanup", "The setup path is correct, but it reads as if every deployment is the same."),
    ("Import edge cases", "A couple of odd records still land with missing attribution fields."),
    ("Weekly support review", "Top tickets are clustered around SSH key rotation and tunnel setup."),
    ("Mobile layout notes", "Rendering stays functional, but the detail pane feels compressed."),
]

COMMENT_SNIPPETS = [
    "I checked this again against the latest build and the behavior is consistent.",
    "We should document the tradeoff explicitly instead of assuming people infer it.",
    "The main risk is rollout timing rather than the code path itself.",
    "I can take the follow-up if we want to keep the decision small and reversible.",
    "This is better after the last pass, but the copy still sounds too careful.",
    "Support will need a shorter canned answer if we ship this as-is.",
    "I tested the unhappy path locally and it failed in a predictable way.",
    "Let's keep the scope narrow and avoid dragging another migration into it.",
]

DM_TOPICS = [
    "Can you take the first pass on the customer-facing note?",
    "I want a cleaner summary before the review thread starts drifting.",
    "Please sanity-check the fallback path before I merge the docs update.",
    "I think the feature is fine, but the release note still oversells it.",
    "If we keep the hotfix narrow, support won't need a new playbook.",
]


def main() -> None:
    parser = argparse.ArgumentParser(description="Populate a local sshoosh SQLite database with demo data.")
    parser.add_argument("--db", default="./sshoosh.sqlite", help="Path to the SQLite database.")
    parser.add_argument("--reset", action="store_true", help="Reset workspace data but preserve the current SSH account and key.")
    parser.add_argument("--owner", help="Username to preserve and promote instead of auto-detecting the latest active account.")
    args = parser.parse_args()

    db_path = Path(args.db)
    db_path.parent.mkdir(parents=True, exist_ok=True)

    with sqlite3.connect(db_path) as conn:
        conn.row_factory = sqlite3.Row
        conn.execute("PRAGMA foreign_keys = ON")
        init_schema(conn)
        anchor = resolve_anchor(conn, args.owner)
        if args.reset:
            reset_preserving_anchor(conn, anchor)
        else:
            cleanup_prior_demo(conn)
        summary = seed_demo_data(conn, anchor)
        conn.commit()

    print(
        f"Seeded demo workspace into {db_path}. "
        f"Anchor account: @{anchor.username}. "
        f"Accounts: {summary['accounts']}. "
        f"Channels: {summary['channels']}. "
        f"Threads: {summary['threads']}. "
        f"Comments: {summary['comments']}. "
        f"Conversations: {summary['conversations']}. "
        f"DM messages: {summary['dm_messages']}."
    )


def init_schema(conn: sqlite3.Connection) -> None:
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS _sshoosh_migrations (
          version TEXT PRIMARY KEY,
          applied_at TEXT NOT NULL
        )
        """
    )
    for version, path in MIGRATIONS:
        exists = conn.execute(
            "SELECT 1 FROM _sshoosh_migrations WHERE version = ?",
            (version,),
        ).fetchone()
        if exists:
            continue
        conn.executescript(path.read_text())
        conn.execute(
            "INSERT INTO _sshoosh_migrations (version, applied_at) VALUES (?, ?)",
            (version, ts(datetime.now(timezone.utc))),
        )


def resolve_anchor(conn: sqlite3.Connection, owner: str | None) -> Account:
    if owner:
        row = conn.execute(
            """
            SELECT id, username, display_name
            FROM accounts
            WHERE username = ? AND disabled_at IS NULL
            """,
            (owner,),
        ).fetchone()
    else:
        row = conn.execute(
            """
            SELECT id, username, display_name
            FROM accounts
            WHERE activated_at IS NOT NULL AND disabled_at IS NULL
            ORDER BY COALESCE(last_seen_at, activated_at, created_at) DESC
            LIMIT 1
            """
        ).fetchone()

    if row is None:
        raise SystemExit("No active account found. Log in once with SSH, then rerun this script.")
    return Account(row["id"], row["username"], row["display_name"])


def reset_preserving_anchor(conn: sqlite3.Connection, anchor: Account) -> None:
    conn.execute("PRAGMA foreign_keys = OFF")
    for table in [
        "search_index",
        "audit_log",
        "event_log",
        "notifications",
        "reactions",
        "mentions",
        "conversation_messages",
        "conversation_members",
        "conversations",
        "thread_reads",
        "comments",
        "threads",
        "channel_members",
        "channels",
        "presence_sessions",
        "invites",
        "bootstrap_tokens",
        "webhook_jobs",
        "server_leases",
    ]:
        delete_if_exists(conn, table)

    conn.execute("DELETE FROM ssh_keys WHERE account_id <> ?", (anchor.id,))
    conn.execute("DELETE FROM accounts WHERE id <> ?", (anchor.id,))
    conn.execute("PRAGMA foreign_keys = ON")


def cleanup_prior_demo(conn: sqlite3.Connection) -> None:
    for table in ["notifications", "reactions", "mentions", "audit_log", "presence_sessions"]:
        conn.execute(f"DELETE FROM {table} WHERE id LIKE 'demo-%'")
    conn.execute("DELETE FROM event_log WHERE kind LIKE 'demo.%'")
    conn.execute("DELETE FROM conversations WHERE id LIKE 'demo-conversation-%'")
    conn.execute("DELETE FROM threads WHERE id LIKE 'demo-thread-%'")
    conn.execute("DELETE FROM channels WHERE id LIKE 'demo-channel-%' AND slug <> 'general'")
    conn.execute("DELETE FROM accounts WHERE id LIKE 'demo-account-%'")
    conn.execute("DELETE FROM search_index")


def delete_if_exists(conn: sqlite3.Connection, table: str) -> None:
    exists = conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type IN ('table', 'view') AND name = ?",
        (table,),
    ).fetchone()
    if exists:
        conn.execute(f"DELETE FROM {table}")


def seed_demo_data(conn: sqlite3.Connection, anchor: Account) -> dict[str, int]:
    now = datetime.now(timezone.utc)
    start = now - timedelta(days=182)
    accounts = {"anchor": anchor}

    conn.execute(
        """
        UPDATE accounts
        SET role = 'owner',
            updated_at = ?,
            activated_at = COALESCE(activated_at, ?),
            last_seen_at = ?
        WHERE id = ?
        """,
        (ts(now), ts(now), ts(now), anchor.id),
    )

    for index, person in enumerate(DEMO_PEOPLE):
        created_at = start + timedelta(days=2 + index * 3)
        seen_at = now - timedelta(hours=index * 7 + 2)
        conn.execute(
            """
            INSERT INTO accounts
              (id, username, display_name, role, settings_json, created_at, updated_at, last_seen_at, activated_at, disabled_at, pending_username)
            VALUES (?, ?, ?, ?, '{}', ?, ?, ?, ?, NULL, NULL)
            ON CONFLICT(id) DO UPDATE SET
              username = excluded.username,
              display_name = excluded.display_name,
              role = excluded.role,
              updated_at = excluded.updated_at,
              last_seen_at = excluded.last_seen_at,
              activated_at = excluded.activated_at,
              disabled_at = NULL,
              pending_username = NULL
            """,
            (person.id, person.username, person.display_name, person.role, ts(created_at), ts(seen_at), ts(seen_at), ts(created_at)),
        )
        accounts[person.username] = Account(person.id, person.username, person.display_name)

    for index, channel in enumerate(CHANNELS):
        created_at = start + timedelta(days=index)
        updated_at = now - timedelta(days=len(CHANNELS) - index)
        existing_general = None
        if channel.slug == "general":
            existing_general = conn.execute("SELECT id FROM channels WHERE slug = 'general'").fetchone()

        if existing_general:
            channel_id = existing_general["id"]
            conn.execute(
                """
                UPDATE channels
                SET name = ?, visibility = 'public', topic = ?, updated_at = ?, archived_at = NULL, archived_by_account_id = NULL
                WHERE id = ?
                """,
                (channel.name, channel.topic, ts(updated_at), channel_id),
            )
        else:
            channel_id = channel.id
            conn.execute(
                """
                INSERT INTO channels
                  (id, slug, name, visibility, topic, created_by_account_id, created_at, updated_at, archived_at, archived_by_account_id)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL)
                ON CONFLICT(id) DO UPDATE SET
                  slug = excluded.slug,
                  name = excluded.name,
                  visibility = excluded.visibility,
                  topic = excluded.topic,
                  updated_at = excluded.updated_at,
                  archived_at = NULL,
                  archived_by_account_id = NULL
                """,
                (channel_id, channel.slug, channel.name, channel.visibility, channel.topic, anchor.id, ts(created_at), ts(updated_at)),
            )

        channel_id = get_channel_id(conn, channel.slug)
        conn.execute("DELETE FROM channel_members WHERE channel_id = ?", (channel_id,))
        for username in channel.members:
            account = accounts[username]
            conn.execute(
                "INSERT INTO channel_members (channel_id, account_id, role, joined_at) VALUES (?, ?, 'member', ?)",
                (channel_id, account.id, ts(created_at)),
            )

    summary = {
        "accounts": len(accounts),
        "channels": len(CHANNELS),
        "threads": 0,
        "comments": 0,
        "conversations": 0,
        "dm_messages": 0,
    }
    comment_counter = 0
    notification_counter = 0
    mention_counter = 0
    reaction_counter = 0

    for week in range(26):
        for lane in range(2):
            channel = CHANNELS[(week + lane) % len(CHANNELS)]
            channel_id = get_channel_id(conn, channel.slug)
            members = [accounts[username] for username in channel.members]
            topic_title, topic_body = THREAD_TOPICS[(week * 2 + lane) % len(THREAD_TOPICS)]
            creator = members[(week + lane) % len(members)]
            created_at = start + timedelta(days=week * 7 + lane * 2)
            comment_count = 4 + ((week + lane) % 4)
            last_activity = created_at + timedelta(hours=comment_count + 1)
            thread_id = f"demo-thread-{summary['threads']:03d}"
            title = f"{topic_title} {week + 1}"
            body = f"{topic_body} Week {week + 1} focus is {channel.topic.lower()}."
            pinned_at = ts(created_at + timedelta(hours=2)) if week % 9 == 0 and lane == 0 else None

            conn.execute(
                """
                INSERT INTO threads
                  (id, channel_id, creator_account_id, title, body, comment_count, last_comment_index, last_activity_at, created_at, updated_at, edited_at, archived_at, pinned_at, deleted_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, ?, NULL)
                """,
                (
                    thread_id,
                    channel_id,
                    creator.id,
                    title,
                    body,
                    comment_count,
                    comment_count,
                    ts(last_activity),
                    ts(created_at),
                    ts(last_activity),
                    pinned_at,
                ),
            )

            for account in members:
                last_read = comment_count if account.id == creator.id else (week + lane + len(account.username)) % (comment_count + 1)
                saved_at = ts(created_at + timedelta(days=1)) if (week + len(account.username)) % 10 == 0 else None
                conn.execute(
                    """
                    INSERT INTO thread_reads
                      (thread_id, account_id, last_read_index, marked_unread_at, muted_until, saved_at)
                    VALUES (?, ?, ?, NULL, NULL, ?)
                    """,
                    (thread_id, account.id, last_read, saved_at),
                )

            conn.execute(
                """
                INSERT INTO event_log
                  (created_at, channel_id, thread_id, conversation_id, kind, payload_json)
                VALUES (?, ?, ?, NULL, 'demo.thread.created', ?)
                """,
                (ts(created_at), channel_id, thread_id, json.dumps({"title": title, "creator": creator.username})),
            )

            for comment_index in range(comment_count):
                author = members[(comment_index + lane + week) % len(members)]
                comment_at = created_at + timedelta(hours=comment_index + 1)
                tail = (
                    f"Closing this pass unless @{anchor.username} sees another blocker."
                    if comment_index == comment_count - 1
                    else f"This affects {channel.slug} directly."
                )
                comment_body = f"{COMMENT_SNIPPETS[(week + comment_index + lane) % len(COMMENT_SNIPPETS)]} {tail}"
                comment_id = f"demo-comment-{comment_counter:04d}"
                conn.execute(
                    """
                    INSERT INTO comments
                      (id, thread_id, channel_id, author_account_id, obj_index, body, created_at, updated_at, edited_at, deleted_at)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL)
                    """,
                    (comment_id, thread_id, channel_id, author.id, comment_index + 1, comment_body, ts(comment_at), ts(comment_at)),
                )

                if comment_index == comment_count - 1 and author.id != anchor.id:
                    mention_id = f"demo-mention-{mention_counter:04d}"
                    notification_id = f"demo-notification-{notification_counter:04d}"
                    conn.execute(
                        """
                        INSERT INTO mentions
                          (id, target_account_id, actor_account_id, source_kind, source_id, channel_id, thread_id, conversation_id, obj_index, created_at, read_at)
                        VALUES (?, ?, ?, 'comment', ?, ?, ?, NULL, ?, ?, NULL)
                        """,
                        (mention_id, anchor.id, author.id, comment_id, channel_id, thread_id, comment_index + 1, ts(comment_at)),
                    )
                    conn.execute(
                        """
                        INSERT INTO notifications
                          (id, account_id, actor_account_id, kind, source_kind, source_id, channel_id, thread_id, conversation_id, title, body, created_at, read_at)
                        VALUES (?, ?, ?, 'mention', 'comment', ?, ?, ?, NULL, ?, ?, ?, NULL)
                        """,
                        (
                            notification_id,
                            anchor.id,
                            author.id,
                            comment_id,
                            channel_id,
                            thread_id,
                            f"{author.display_name} mentioned you in #{channel.slug}",
                            comment_body,
                            ts(comment_at),
                        ),
                    )
                    mention_counter += 1
                    notification_counter += 1

                if comment_index % 2 == 0:
                    reactor = members[(comment_index + 1) % len(members)]
                    conn.execute(
                        """
                        INSERT INTO reactions
                          (id, source_kind, source_id, account_id, emoji, created_at)
                        VALUES (?, 'comment', ?, ?, ?, ?)
                        """,
                        (
                            f"demo-reaction-{reaction_counter:04d}",
                            comment_id,
                            reactor.id,
                            "👀" if comment_index % 4 == 0 else "👍",
                            ts(comment_at + timedelta(minutes=10)),
                        ),
                    )
                    reaction_counter += 1

                comment_counter += 1
                summary["comments"] += 1

            summary["threads"] += 1

    dm_pairs = [
        ("anchor", "marco"),
        ("anchor", "nina"),
        ("anchor", "sam"),
        ("anchor", "maya"),
        ("marco", "sam"),
        ("marco", "priya"),
        ("nina", "jules"),
        ("sam", "ivy"),
        ("zoe", "maya"),
        ("omar", "luca"),
        ("anchor", "zoe"),
        ("anchor", "omar"),
    ]
    for conversation_index, (left_name, right_name) in enumerate(dm_pairs):
        left = accounts[left_name]
        right = accounts[right_name]
        created_at = start + timedelta(days=5 + conversation_index * 11)
        conversation_id = f"demo-conversation-{conversation_index:03d}"
        message_count = 8 + conversation_index % 5
        conn.execute(
            """
            INSERT INTO conversations
              (id, dm_key, creator_account_id, last_message_index, last_activity_at, created_at, archived_at)
            VALUES (?, ?, ?, ?, ?, ?, NULL)
            """,
            (
                conversation_id,
                dm_key(left.username, right.username),
                left.id,
                message_count,
                ts(created_at + timedelta(hours=message_count)),
                ts(created_at),
            ),
        )
        for member in [left, right]:
            saved_at = ts(created_at + timedelta(days=2)) if conversation_index % 4 == 0 else None
            conn.execute(
                """
                INSERT INTO conversation_members
                  (conversation_id, account_id, joined_at, last_read_index, muted_until, saved_at)
                VALUES (?, ?, ?, ?, NULL, ?)
                """,
                (conversation_id, member.id, ts(created_at), message_count, saved_at),
            )
        conn.execute(
            """
            INSERT INTO event_log
              (created_at, channel_id, thread_id, conversation_id, kind, payload_json)
            VALUES (?, NULL, NULL, ?, 'demo.conversation.created', ?)
            """,
            (ts(created_at), conversation_id, json.dumps({"participants": [left.username, right.username]})),
        )
        for message_index in range(message_count):
            author = left if message_index % 2 == 0 else right
            message_at = created_at + timedelta(hours=message_index)
            message_id = f"demo-dm-{conversation_index:03d}-{message_index:03d}"
            tail = (
                "If that looks good, I'll post the final version in-channel."
                if message_index == message_count - 1
                else "Keeping this off the main thread for now."
            )
            body = f"{DM_TOPICS[(conversation_index + message_index) % len(DM_TOPICS)]} {tail}"
            conn.execute(
                """
                INSERT INTO conversation_messages
                  (id, conversation_id, author_account_id, obj_index, body, created_at, updated_at, edited_at, deleted_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, NULL, NULL)
                """,
                (message_id, conversation_id, author.id, message_index + 1, body, ts(message_at), ts(message_at)),
            )
            if author.id != anchor.id and message_index == 0:
                conn.execute(
                    """
                    INSERT INTO notifications
                      (id, account_id, actor_account_id, kind, source_kind, source_id, channel_id, thread_id, conversation_id, title, body, created_at, read_at)
                    VALUES (?, ?, ?, 'dm', 'dm', ?, NULL, NULL, ?, ?, ?, ?, ?)
                    """,
                    (
                        f"demo-notification-dm-{conversation_index:03d}",
                        anchor.id,
                        author.id,
                        message_id,
                        conversation_id,
                        f"New DM from {author.display_name}",
                        body,
                        ts(message_at),
                        ts(message_at + timedelta(hours=3)),
                    ),
                )
            summary["dm_messages"] += 1
        summary["conversations"] += 1

    for username in ["anchor", "marco", "sam", "maya"]:
        account = accounts[username]
        conn.execute(
            """
            INSERT INTO presence_sessions
              (id, account_id, started_at, last_seen_at, disconnected_at, node_id)
            VALUES (?, ?, ?, ?, NULL, ?)
            """,
            (
                f"demo-presence-{account.username}",
                account.id,
                ts(now - timedelta(minutes=40 + len(account.username) * 3)),
                ts(now - timedelta(minutes=len(account.username))),
                "demo-node",
            ),
        )

    conn.execute(
        """
        INSERT INTO audit_log
          (id, actor_account_id, action, target, metadata_json, created_at)
        VALUES (?, ?, 'demo.seeded', 'workspace', ?, ?)
        """,
        (
            "demo-audit-seed",
            anchor.id,
            json.dumps(
                {
                    "anchor_username": anchor.username,
                    "threads": summary["threads"],
                    "comments": summary["comments"],
                    "conversations": summary["conversations"],
                    "dm_messages": summary["dm_messages"],
                }
            ),
            ts(now),
        ),
    )
    rebuild_search_index(conn)
    return summary


def get_channel_id(conn: sqlite3.Connection, slug: str) -> str:
    row = conn.execute("SELECT id FROM channels WHERE slug = ?", (slug,)).fetchone()
    if row is None:
        raise RuntimeError(f"missing channel #{slug}")
    return row["id"]


def rebuild_search_index(conn: sqlite3.Connection) -> None:
    conn.execute("DELETE FROM search_index")
    conn.execute(
        """
        INSERT INTO search_index
          (kind, object_id, channel_id, thread_id, conversation_id, title, body, context)
        SELECT 'thread', t.id, t.channel_id, t.id, NULL, t.title, t.body, '#' || c.slug
        FROM threads t
        JOIN channels c ON c.id = t.channel_id
        WHERE t.deleted_at IS NULL
        """
    )
    conn.execute(
        """
        INSERT INTO search_index
          (kind, object_id, channel_id, thread_id, conversation_id, title, body, context)
        SELECT 'comment', cm.id, cm.channel_id, cm.thread_id, NULL, t.title, cm.body, '#' || c.slug
        FROM comments cm
        JOIN threads t ON t.id = cm.thread_id
        JOIN channels c ON c.id = cm.channel_id
        WHERE cm.deleted_at IS NULL AND t.deleted_at IS NULL
        """
    )
    conn.execute(
        """
        INSERT INTO search_index
          (kind, object_id, channel_id, thread_id, conversation_id, title, body, context)
        SELECT 'dm', m.id, NULL, NULL, m.conversation_id, 'DM', m.body, 'DM'
        FROM conversation_messages m
        WHERE m.deleted_at IS NULL
        """
    )


def dm_key(left: str, right: str) -> str:
    return f"{left}:{right}" if left <= right else f"{right}:{left}"


def ts(value: datetime) -> str:
    return value.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")


if __name__ == "__main__":
    main()
