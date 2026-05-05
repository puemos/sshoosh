#!/usr/bin/env python3
"""Seed a local sshoosh SQLite database with rich, realistic demo data."""

from __future__ import annotations

import argparse
import json
import re
import sqlite3
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

# The baseline migration is kept current in this repository. The extra entries
# here are additive migrations that are safe to apply from a standalone script.
MIGRATIONS = [
    ("20260430000000_initial", ROOT / "migrations/20260430000000_initial.sql"),
    ("20260430000001_pending_username", ROOT / "migrations/20260430000001_pending_username.sql"),
    ("20260430000001_remote_security", ROOT / "migrations/20260430000001_remote_security.sql"),
    ("20260501000000_saved_messages", ROOT / "migrations/20260501000000_saved_messages.sql"),
    ("20260501000001_notification_archive", ROOT / "migrations/20260501000001_notification_archive.sql"),
    ("20260501000003_dm_sidebar_scale", ROOT / "migrations/20260501000003_dm_sidebar_scale.sql"),
    ("20260501000004_device_link_tokens", ROOT / "migrations/20260501000004_device_link_tokens.sql"),
    ("20260501000005_message_labels", ROOT / "migrations/20260501000005_message_labels.sql"),
    ("20260501000006_query_performance", ROOT / "migrations/20260501000006_query_performance.sql"),
]

LABEL_RE = re.compile(r"(?<![A-Za-z0-9_-])\$([A-Za-z0-9_-]+)")
MENTION_RE = re.compile(r"(?<![A-Za-z0-9_.-])@([A-Za-z0-9_.-]+)")


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
    title: str
    team: str
    location: str


@dataclass(frozen=True)
class Channel:
    id: str
    slug: str
    name: str
    visibility: str
    topic: str
    members: tuple[str, ...]


@dataclass(frozen=True)
class CommentSeed:
    author: str
    body: str
    minutes_after: int
    edited: bool = False
    reactions: tuple[tuple[str, str], ...] = ()
    saved_by: tuple[str, ...] = ()


@dataclass(frozen=True)
class ThreadScenario:
    channel_slug: str
    title: str
    body: str
    creator: str
    days_ago: int
    comments: tuple[CommentSeed, ...]
    pinned: bool = False
    archived: bool = False
    edited: bool = False
    reactions: tuple[tuple[str, str], ...] = ()
    saved_by: tuple[str, ...] = ()
    muted_for: tuple[str, ...] = ()
    unread_by: tuple[tuple[str, int], ...] = ()
    generated_comments: int = 0
    generator: str = "weekly"


@dataclass(frozen=True)
class DmMessageSeed:
    author: str
    body: str
    minutes_after: int
    edited: bool = False
    reactions: tuple[tuple[str, str], ...] = ()
    saved_by: tuple[str, ...] = ()


@dataclass(frozen=True)
class DmScenario:
    left: str
    right: str
    days_ago: int
    messages: tuple[DmMessageSeed, ...]
    saved_by: tuple[str, ...] = ()
    muted_by: tuple[str, ...] = ()
    unread_by: tuple[tuple[str, int], ...] = ()


DEMO_PEOPLE = [
    DemoPerson("demo-account-marco", "marco", "Marco Bellini", "admin", "Infrastructure Lead", "engineering", "Milan"),
    DemoPerson("demo-account-nina", "nina", "Nina Patel", "member", "Product Lead", "product", "London"),
    DemoPerson("demo-account-sam", "sam", "Sam Rivera", "member", "Support Engineer", "support", "Austin"),
    DemoPerson("demo-account-priya", "priya", "Priya Shah", "member", "Platform Engineer", "engineering", "Berlin"),
    DemoPerson("demo-account-jules", "jules", "Jules Martin", "member", "Product Designer", "design", "Paris"),
    DemoPerson("demo-account-zoe", "zoe", "Zoe Kim", "member", "Customer Success", "customers", "Seoul"),
    DemoPerson("demo-account-omar", "omar", "Omar Haddad", "member", "SRE", "ops", "Toronto"),
    DemoPerson("demo-account-ivy", "ivy", "Ivy Chen", "member", "Security Engineer", "security", "Singapore"),
    DemoPerson("demo-account-luca", "luca", "Luca Rossi", "member", "Developer Advocate", "docs", "Rome"),
    DemoPerson("demo-account-maya", "maya", "Maya Johnson", "member", "Operations Lead", "ops", "New York"),
    DemoPerson("demo-account-ellis", "ellis", "Ellis Grant", "member", "Data Analyst", "product", "Dublin"),
    DemoPerson("demo-account-rhea", "rhea", "Rhea Kapoor", "member", "Solutions Architect", "customers", "Mumbai"),
]

CHANNELS = [
    Channel(
        "demo-channel-general",
        "general",
        "general",
        "public",
        "Company-wide operator briefings, handoffs, and default coordination",
        ("anchor", "marco", "nina", "sam", "priya", "jules", "zoe", "omar", "ivy", "luca", "maya", "ellis", "rhea"),
    ),
    Channel(
        "demo-channel-ops",
        "ops",
        "ops",
        "public",
        "Deploy windows, leases, backups, incidents, and on-call handoffs",
        ("anchor", "marco", "sam", "priya", "omar", "ivy", "luca", "maya", "rhea"),
    ),
    Channel(
        "demo-channel-engineering",
        "engineering",
        "engineering",
        "public",
        "Rust services, SQLite/libSQL, SSH transport, search, and release engineering",
        ("anchor", "marco", "sam", "priya", "omar", "ivy", "luca", "ellis"),
    ),
    Channel(
        "demo-channel-product",
        "product",
        "product",
        "public",
        "Planning, customer signal, activation, and weekly product review",
        ("anchor", "marco", "nina", "jules", "maya", "ellis", "rhea"),
    ),
    Channel(
        "demo-channel-design",
        "design",
        "design",
        "public",
        "Terminal UI density, interaction polish, copy, and onboarding fit-and-finish",
        ("anchor", "nina", "jules", "zoe", "luca", "maya"),
    ),
    Channel(
        "demo-channel-support",
        "support",
        "support",
        "public",
        "Customer issues, triage, escalation summaries, and incident follow-up",
        ("anchor", "sam", "zoe", "ivy", "luca", "maya", "rhea", "ellis"),
    ),
    Channel(
        "demo-channel-customers",
        "customers",
        "customers",
        "public",
        "Beta cohort feedback, field notes, and customer-facing decisions",
        ("anchor", "nina", "sam", "zoe", "luca", "maya", "ellis", "rhea"),
    ),
    Channel(
        "demo-channel-security",
        "security",
        "security",
        "private",
        "Private threat modeling, token handling, and hardening reviews",
        ("anchor", "marco", "priya", "omar", "ivy", "maya"),
    ),
    Channel(
        "demo-channel-launch",
        "launch-room",
        "launch-room",
        "private",
        "Private release coordination, customer comms, and rollout tracking",
        ("anchor", "marco", "nina", "sam", "priya", "zoe", "luca", "maya"),
    ),
    Channel(
        "demo-channel-leadership",
        "leadership",
        "leadership",
        "private",
        "Hiring, budget, support load, and staffing decisions",
        ("anchor", "marco", "nina", "maya", "ellis"),
    ),
]

SHOWCASE_THREADS = [
    ThreadScenario(
        channel_slug="general",
        title="Monday operator brief: release week is live",
        body=(
            "Release week is live. Focus areas are installer verification, customer-ready support answers, "
            "and keeping the SSH onboarding path calm under pressure. $release $brief"
        ),
        creator="maya",
        days_ago=1,
        pinned=True,
        reactions=(("marco", "🚀"), ("nina", "✅"), ("anchor", "👀")),
        saved_by=("anchor",),
        unread_by=(("anchor", 3),),
        comments=(
            CommentSeed(
                "marco",
                "Linux and macOS artifacts are reproducible. I still want one clean install from a blank VM before we call this green. $release",
                35,
                reactions=(("priya", "✅"), ("luca", "👀")),
            ),
            CommentSeed(
                "sam",
                "Support has the short answers ready for key rotation, token redemption, and restore checks. I added the escalation trigger to $support.",
                84,
                reactions=(("maya", "✅"),),
            ),
            CommentSeed(
                "nina",
                "The customer note now says what operators can do in the first five minutes instead of listing every feature.",
                129,
                edited=True,
                reactions=(("jules", "✅"),),
                saved_by=("anchor",),
            ),
            CommentSeed(
                "maya",
                "@{anchor} please use this as the opening thread when you demo the workspace. It has the launch, support, and ops context in one place.",
                188,
                reactions=(("anchor", "👍"), ("zoe", "💯")),
            ),
        ),
    ),
    ThreadScenario(
        channel_slug="support",
        title="P0 Acme import backlog after SSH key rotation",
        body=(
            "Acme rotated deploy keys during their maintenance window and two import workers stalled on stale fingerprints. "
            "Need customer update, replay plan, and owner for post-incident notes. $incident $customer-acme $p0"
        ),
        creator="sam",
        days_ago=2,
        pinned=True,
        reactions=(("zoe", "👀"), ("ivy", "✅")),
        saved_by=("anchor", "sam"),
        muted_for=("maya",),
        unread_by=(("anchor", 4), ("zoe", 2)),
        comments=(
            CommentSeed(
                "rhea",
                "Their new key is valid. The stall happened because the old fingerprint remained in the worker cache for 19 minutes. $root-cause",
                22,
                reactions=(("ivy", "👀"),),
            ),
            CommentSeed(
                "ivy",
                "No token exposure. Logs show only fingerprint hashes and the keyboard-interactive prompt never accepted anything through auth_password.",
                47,
                saved_by=("anchor",),
                reactions=(("sam", "✅"), ("marco", "✅")),
            ),
            CommentSeed(
                "zoe",
                "Customer-facing copy: imports are replaying, duplicate protection is on, and we will send the final count before their EOD. $customer-acme",
                96,
                edited=True,
                reactions=(("nina", "✅"),),
            ),
            CommentSeed(
                "sam",
                "@{anchor} the replay is at 82 percent and the only remaining risk is the oldest attachment batch. I will keep the incident label until final export validates.",
                141,
                reactions=(("anchor", "👀"), ("rhea", "👍")),
            ),
            CommentSeed(
                "ellis",
                "Dashboard check: 31,442 source rows, 31,442 indexed rows, 0 duplicate thread keys. Export checksum matches the dry run.",
                205,
                saved_by=("sam",),
                reactions=(("zoe", "✅"),),
            ),
        ),
    ),
    ThreadScenario(
        channel_slug="engineering",
        title="Draft recovery after suspended SSH sessions",
        body=(
            "Users on unstable tunnels can suspend the terminal, reconnect, and lose an unsent composer draft. "
            "Proposal: keep per-session draft state until submit, clear, or account switch. $ux $reliability"
        ),
        creator="priya",
        days_ago=3,
        reactions=(("anchor", "👀"), ("marco", "✅")),
        saved_by=("anchor",),
        unread_by=(("anchor", 2),),
        comments=(
            CommentSeed(
                "omar",
                "I reproduced it by suspending the SSH client mid-compose, waiting for the heartbeat timeout, then reconnecting through the same key.",
                31,
                reactions=(("priya", "👀"),),
            ),
            CommentSeed(
                "marco",
                "Keep it session-scoped. Durable drafts sound nice, but they create privacy questions for shared terminals. $decision",
                74,
                saved_by=("anchor",),
                reactions=(("ivy", "✅"), ("nina", "✅")),
            ),
            CommentSeed(
                "jules",
                "UI note: do not flash a recovery banner on every reconnect. Only show it when the restored draft is non-empty.",
                131,
                edited=True,
                reactions=(("nina", "✅"),),
            ),
            CommentSeed(
                "priya",
                "@{anchor} I have the service boundary clean: TUI owns the buffer, SSH session owns lifecycle, no persistence write needed.",
                195,
                reactions=(("anchor", "👍"), ("omar", "✅")),
            ),
        ),
    ),
    ThreadScenario(
        channel_slug="product",
        title="First 90 seconds: bootstrap, invite, first channel",
        body=(
            "The activation path should prove value before asking teams to understand every admin command. "
            "Target flow: redeem token, land in #general, send one thread, invite teammate. $activation $onboarding"
        ),
        creator="nina",
        days_ago=4,
        reactions=(("jules", "👀"), ("anchor", "✅")),
        saved_by=("anchor", "nina"),
        comments=(
            CommentSeed(
                "rhea",
                "Field calls show people understand SSH keys faster when the product tells them who invited them and where they landed.",
                28,
                reactions=(("nina", "✅"),),
            ),
            CommentSeed(
                "jules",
                "The first screen should feel like a working room, not a tour. Put the composer in focus and let the sidebar explain itself.",
                67,
                saved_by=("anchor",),
                reactions=(("nina", "💯"),),
            ),
            CommentSeed(
                "luca",
                "Docs can mirror that path: one command to run, one SSH command to connect, one invite command after success. $docs",
                118,
                reactions=(("rhea", "✅"),),
            ),
            CommentSeed(
                "nina",
                "@{anchor} this is the demo storyline I would use: operator joins, sees live work, jumps to a source link, saves the decision.",
                169,
                reactions=(("anchor", "👀"),),
            ),
        ),
    ),
    ThreadScenario(
        channel_slug="design",
        title="Narrow terminal polish pass",
        body=(
            "At 80 columns the app is usable, but metadata competes with message bodies. Tighten badges, truncate source labels, "
            "and keep hover targets stable. $tui $design"
        ),
        creator="jules",
        days_ago=5,
        reactions=(("nina", "✅"), ("maya", "👀")),
        comments=(
            CommentSeed(
                "jules",
                "Thread rows should hold title, unread count, muted/saved state, and timestamp without pushing the label into a second visual rhythm.",
                26,
                reactions=(("nina", "✅"),),
            ),
            CommentSeed(
                "luca",
                "Command help is now scannable at 80 columns. Long descriptions wrap under the command name instead of colliding with aliases.",
                63,
                reactions=(("jules", "👍"),),
            ),
            CommentSeed(
                "maya",
                "Please keep destructive confirmation visible in the same row. Operators should not have to search the screen before deleting.",
                122,
                saved_by=("anchor",),
                reactions=(("ivy", "✅"),),
            ),
        ),
    ),
    ThreadScenario(
        channel_slug="security",
        title="Keyboard-interactive token redemption review",
        body=(
            "Threat model pass for unknown SSH keys. Tokens must only be accepted through keyboard-interactive, never username parsing "
            "or auth_password. $security $tokens $decision"
        ),
        creator="ivy",
        days_ago=6,
        pinned=True,
        reactions=(("marco", "✅"), ("priya", "✅"), ("anchor", "👀")),
        saved_by=("anchor", "ivy"),
        unread_by=(("anchor", 1),),
        comments=(
            CommentSeed(
                "ivy",
                "Validated prompts: bootstrap token, invite token, and device link token all stay out of account rows until redemption succeeds.",
                21,
                reactions=(("priya", "✅"),),
            ),
            CommentSeed(
                "priya",
                "The SSH transport only hands sanitized challenge responses to the service layer. No persistence happens before service approval.",
                79,
                saved_by=("anchor",),
                reactions=(("ivy", "✅"), ("marco", "✅")),
            ),
            CommentSeed(
                "omar",
                "Log review is clean. Failed redemption records include reason class and actor fingerprint, not submitted token value.",
                136,
                reactions=(("ivy", "👀"),),
            ),
            CommentSeed(
                "ivy",
                "@{anchor} I am marking this accepted with one follow-up: add a CLI doctor check for server key permissions. $security",
                211,
                reactions=(("anchor", "👍"),),
            ),
        ),
    ),
    ThreadScenario(
        channel_slug="launch-room",
        title="Launch checklist: installer, Homebrew tap, GHCR",
        body=(
            "Private launch room for the final release train. Keep status terse: artifact, owner, risk, next checkpoint. "
            "$launch $release $checklist"
        ),
        creator="marco",
        days_ago=7,
        pinned=True,
        reactions=(("nina", "🚀"), ("sam", "👀"), ("anchor", "✅")),
        saved_by=("anchor", "marco"),
        unread_by=(("anchor", 5), ("sam", 2)),
        comments=(
            CommentSeed(
                "luca",
                "Installer dry run passes on a clean machine. The checksum copy is shorter and no longer implies curl is mandatory. $docs",
                25,
                reactions=(("nina", "✅"),),
            ),
            CommentSeed(
                "priya",
                "Release build is green. Cargo.lock is current and the SQLite feature set matches the packaged binary.",
                58,
                reactions=(("marco", "✅"),),
            ),
            CommentSeed(
                "zoe",
                "Customer comms are ready for beta admins. I kept the promise narrow: self-hosted SSH chat, local data, fast recovery.",
                109,
                saved_by=("anchor",),
                reactions=(("sam", "✅"),),
            ),
            CommentSeed(
                "sam",
                "Support macros are staged. The only open item is the answer for existing users who want to rotate host keys.",
                164,
                reactions=(("ivy", "👀"),),
            ),
            CommentSeed(
                "marco",
                "@{anchor} next checkpoint is tomorrow 09:00 UTC. I will post artifacts, tap diff, GHCR tags, and SHA256 sums in this thread.",
                233,
                reactions=(("anchor", "👀"), ("maya", "✅")),
            ),
        ),
    ),
    ThreadScenario(
        channel_slug="customers",
        title="Beta cohort signal: source links beat dashboards",
        body=(
            "Five beta teams said source navigation is the moment the product clicks: notification -> source -> context -> reply. "
            "We should lead demos with that loop. $customer-signal $activation"
        ),
        creator="zoe",
        days_ago=8,
        reactions=(("nina", "💯"), ("rhea", "✅")),
        comments=(
            CommentSeed(
                "ellis",
                "Measured sessions agree. Teams that use source links in the first day create 2.4x more threads by day three.",
                38,
                reactions=(("nina", "👀"),),
            ),
            CommentSeed(
                "rhea",
                "Operators describe it as less context switching, not as search. That distinction matters for positioning.",
                96,
                saved_by=("anchor",),
                reactions=(("zoe", "✅"),),
            ),
            CommentSeed(
                "nina",
                "Agree. We should avoid dashboard language and show the actual workflow: mention, unread state, source jump, saved decision.",
                153,
                reactions=(("jules", "✅"),),
            ),
        ),
    ),
    ThreadScenario(
        channel_slug="engineering",
        title="FTS ranking and label feed polish",
        body=(
            "Search recall is strong, but ranking still overweights old import threads. Labels should make current work easier to find. "
            "$search $labels $performance"
        ),
        creator="ellis",
        days_ago=10,
        reactions=(("priya", "👀"), ("anchor", "✅")),
        saved_by=("anchor",),
        comments=(
            CommentSeed(
                "ellis",
                "Recent comments should beat old thread bodies when the query is a customer name. That fixes most support searches.",
                33,
                reactions=(("sam", "✅"),),
            ),
            CommentSeed(
                "priya",
                "I added a search_documents mapping so rowids stay stable through FTS rebuilds. That also makes deletion cheaper. $performance",
                77,
                saved_by=("anchor",),
                reactions=(("marco", "✅"),),
            ),
            CommentSeed(
                "luca",
                "Docs now use $labels examples with dollar tags, not hashtags. The command help and examples match.",
                126,
                reactions=(("ellis", "✅"),),
            ),
            CommentSeed(
                "ellis",
                "@{anchor} the hot labels for this demo should show launch, incident, support, security, and customer-acme near the top.",
                188,
                reactions=(("anchor", "👀"),),
            ),
        ),
    ),
    ThreadScenario(
        channel_slug="leadership",
        title="May coverage and hiring tradeoffs",
        body=(
            "Private planning for support load, on-call sustainability, and the next hire. Keep the decision notes concise. "
            "$planning $hiring"
        ),
        creator="maya",
        days_ago=12,
        reactions=(("marco", "👀"), ("nina", "✅")),
        saved_by=("anchor", "maya"),
        comments=(
            CommentSeed(
                "ellis",
                "Support volume is spiky, not steadily rising. Two launch windows drove 61 percent of last month's escalations.",
                46,
                reactions=(("maya", "👀"),),
            ),
            CommentSeed(
                "nina",
                "Product can remove two repeated support questions by tightening token redemption copy and the first-run success state.",
                104,
                reactions=(("maya", "✅"),),
            ),
            CommentSeed(
                "marco",
                "Hiring recommendation: SRE first if remote libSQL adoption keeps growing, support engineer first if launch volume doubles.",
                151,
                saved_by=("anchor",),
                reactions=(("maya", "✅"),),
            ),
        ),
    ),
    ThreadScenario(
        channel_slug="ops",
        title="Incident review: remote lease failover rehearsal",
        body=(
            "Failover rehearsal exposed one confusing standby message but no data corruption. Closing this as accepted with docs follow-up. "
            "$incident $reliability $closed"
        ),
        creator="omar",
        days_ago=17,
        archived=True,
        reactions=(("marco", "✅"), ("ivy", "✅")),
        comments=(
            CommentSeed(
                "omar",
                "Master lease moved from node-a to node-b in 4.8 seconds. Fencing token advanced exactly once.",
                33,
                reactions=(("priya", "✅"),),
            ),
            CommentSeed(
                "ivy",
                "Audit log preserved the standby write rejection and did not include auth tokens. Security review has no blocker.",
                91,
                saved_by=("anchor",),
                reactions=(("omar", "✅"),),
            ),
            CommentSeed(
                "luca",
                "Docs follow-up is merged. I renamed the section from cluster mode to active/standby so operators do not overread it.",
                147,
                reactions=(("maya", "✅"),),
            ),
        ),
    ),
]

WEEKLY_TEMPLATES = [
    (
        "ops",
        "Weekly reliability review {week}: leases, backups, restore",
        "Routine operator review for leases, backups, restore proof, and deploy window hygiene. $weekly $reliability",
        "omar",
        ("omar", "maya", "marco", "ivy"),
        (
            "Lease heartbeat stayed inside SLO. The only blip was a standby node restarting during log rotation.",
            "Backup sample restored in staging and matched the checksum from the previous night.",
            "Next week I want the restore note linked from the deploy checklist. @{anchor} may want this visible in the demo.",
        ),
    ),
    (
        "engineering",
        "Engineering triage {week}: Rust service and SQLite edges",
        "Weekly triage for service boundaries, query shape, SSH session glue, and regression risk. $engineering $triage",
        "priya",
        ("priya", "marco", "ellis", "luca"),
        (
            "The service layer stayed clean; no persistence rule moved into render or SSH transport.",
            "One query needs an index before the next import-size demo, but the current data set is responsive.",
            "I tagged the follow-up as $performance so it shows up in hot labels.",
        ),
    ),
    (
        "product",
        "Product review {week}: activation and saved decisions",
        "Weekly product review for activation, source navigation, saved messages, and customer language. $weekly $activation",
        "nina",
        ("nina", "jules", "ellis", "rhea"),
        (
            "Activation notes keep coming back to one pattern: people want to reply before they configure.",
            "Saved messages are acting like lightweight decisions, not bookmarks. That should guide copy.",
            "Customer calls prefer examples with real operator work instead of feature inventory.",
        ),
    ),
    (
        "support",
        "Support digest {week}: key rotation, exports, imports",
        "Weekly digest of customer issues by theme, severity, and owner. $support $weekly",
        "sam",
        ("sam", "zoe", "rhea", "ellis"),
        (
            "Top question is still SSH key rotation. The shorter macro reduced back-and-forth on three tickets.",
            "Export requests mostly come after incident reviews, which means source links should be included in the answer.",
            "I am keeping $customer-signal on this because the same confusion appeared in two beta accounts.",
        ),
    ),
    (
        "design",
        "Design desk check {week}: dense terminal affordances",
        "Weekly design desk check for compact rows, hover targets, empty states, and destructive confirmations. $design $tui",
        "jules",
        ("jules", "nina", "luca", "maya"),
        (
            "The dense layout works when the selected state is unmistakable and badges do not move the row height.",
            "Empty states should be quiet. The product is strongest when the workspace itself is the explanation.",
            "I left a $decision note on the confirmation row so the behavior is easy to find later.",
        ),
    ),
    (
        "customers",
        "Beta field notes {week}: operator workflows",
        "Weekly field-note rollup from beta teams and customer calls. $customer-signal $weekly",
        "zoe",
        ("zoe", "rhea", "nina", "ellis"),
        (
            "Teams like that public channels are discoverable while private content stays invisible until membership changes.",
            "The most repeated compliment is speed: SSH in, read, reply, move on.",
            "The most repeated ask is clearer language around invite and bootstrap token ownership.",
        ),
    ),
]

DM_SCENARIOS = [
    DmScenario(
        "anchor",
        "marco",
        1,
        (
            DmMessageSeed("marco", "Launch-room thread is ready. I kept the release risk list short so it reads like an operator handoff. $launch", 18),
            DmMessageSeed("anchor", "Good. Please keep the Homebrew tap status visible; it is the easiest artifact for people to understand.", 42),
            DmMessageSeed("marco", "Agreed. I will post tap diff, release assets, GHCR tags, and SHA256 sums in one comment.", 76, saved_by=("anchor",)),
            DmMessageSeed("anchor", "Also leave the standby lease note in #ops. It shows the self-hosted story without a slide.", 109),
            DmMessageSeed("marco", "@{anchor} done. I left it as a source link from the launch checklist. $decision", 154, reactions=(("anchor", "✅"),)),
        ),
        saved_by=("anchor",),
        unread_by=(("anchor", 1),),
    ),
    DmScenario(
        "anchor",
        "sam",
        2,
        (
            DmMessageSeed("sam", "Acme wants a plain-English summary before their admin signs off. I can send the replay count and key-cache explanation. $customer-acme", 21),
            DmMessageSeed("anchor", "Use the support thread wording. No need to mention implementation details unless they ask.", 51),
            DmMessageSeed("sam", "Copy: imports replayed cleanly, duplicates blocked, old key cache expired, final export attached.", 88, saved_by=("anchor",)),
            DmMessageSeed("anchor", "That works. Please keep the incident thread updated when the attachment batch finishes.", 119),
            DmMessageSeed("sam", "@{anchor} final batch is done. Customer confirmed the row count. $incident", 177, reactions=(("anchor", "🎉"),)),
        ),
        unread_by=(("anchor", 1),),
    ),
    DmScenario(
        "anchor",
        "nina",
        4,
        (
            DmMessageSeed("nina", "For the demo, I would not start with admin. Start with a mention and jump to source. $activation", 27),
            DmMessageSeed("anchor", "Agreed. The product feels strongest when the first action is reading real work.", 62),
            DmMessageSeed("nina", "Then show saved decisions and hot labels. It makes the terminal feel organized instead of busy.", 118, saved_by=("anchor",)),
            DmMessageSeed("anchor", "Send me the final three-beat narrative when you have it.", 164),
            DmMessageSeed("nina", "Mention -> source -> saved decision. Then invite teammate. That is the full loop.", 231, reactions=(("anchor", "✅"),)),
        ),
        saved_by=("anchor", "nina"),
    ),
    DmScenario(
        "anchor",
        "ivy",
        5,
        (
            DmMessageSeed("ivy", "Security review is clean, but I want the demo to mention that tokens never travel through auth_password. $security", 34),
            DmMessageSeed("anchor", "Yes, but keep it concise. The invariant matters; the full threat model can stay in #security.", 91),
            DmMessageSeed("ivy", "I will phrase it as: unknown keys must redeem through the prompt before any account row exists.", 128, saved_by=("anchor",)),
            DmMessageSeed("anchor", "Perfect. That is specific and operator-friendly.", 173, reactions=(("ivy", "✅"),)),
        ),
        saved_by=("anchor",),
    ),
    DmScenario(
        "nina",
        "jules",
        6,
        (
            DmMessageSeed("nina", "Can you tighten the empty state copy? It still reads like a tour.", 19),
            DmMessageSeed("jules", "Yes. I am making it quieter and keeping commands out of the visual explanation. $design", 45),
            DmMessageSeed("nina", "Good. The app should look useful before it starts explaining itself.", 91, reactions=(("jules", "💯"),)),
            DmMessageSeed("jules", "Updated. The first viewport is just the workspace: channels, DMs, saved, notifications.", 138),
        ),
        saved_by=("nina",),
    ),
    DmScenario(
        "marco",
        "priya",
        8,
        (
            DmMessageSeed("marco", "Can you sanity-check the FTS rebuild? I do not want rowids drifting after deletes. $search", 22),
            DmMessageSeed("priya", "Already checked. search_documents owns the rowid and FTS is rebuilt from that mapping.", 67),
            DmMessageSeed("marco", "Good. Please add it to the engineering thread so support can find it later.", 103),
            DmMessageSeed("priya", "Posted with $performance and $labels. The source link should be enough.", 166, reactions=(("marco", "✅"),)),
        ),
    ),
    DmScenario(
        "sam",
        "zoe",
        9,
        (
            DmMessageSeed("zoe", "Acme asked whether replaying imports could duplicate comments.", 24),
            DmMessageSeed("sam", "Answer is no. We dedupe by source identity and verify final counts before export. $customer-acme", 61),
            DmMessageSeed("zoe", "Thanks. I will keep the customer update focused on final counts and timing.", 119, saved_by=("sam",)),
            DmMessageSeed("sam", "Good. Mention me if they ask for technical detail.", 171),
        ),
    ),
    DmScenario(
        "omar",
        "luca",
        13,
        (
            DmMessageSeed("omar", "The failover docs still say cluster mode. That makes the setup sound heavier than it is.", 33),
            DmMessageSeed("luca", "Agreed. I will rename it active/standby and keep the env vars near the command. $docs", 76),
            DmMessageSeed("omar", "Please include stable SSHOOSH_NODE_ID. That was the only thing people missed in rehearsal.", 132),
            DmMessageSeed("luca", "Done. Also linked the doctor check section from release notes.", 194, reactions=(("omar", "✅"),)),
        ),
        muted_by=("omar",),
    ),
    DmScenario(
        "anchor",
        "maya",
        15,
        (
            DmMessageSeed("maya", "Leadership thread has the hiring tradeoff. I kept names out and left only capacity numbers. $hiring", 31),
            DmMessageSeed("anchor", "Thanks. Please save the support-volume comment so it is easy to find in planning.", 79),
            DmMessageSeed("maya", "Saved. Also muting the noisy weekly support digest until launch is done.", 134, saved_by=("anchor",)),
            DmMessageSeed("anchor", "Makes sense. Keep the launch-room checklist as the source of truth.", 197),
        ),
        saved_by=("anchor", "maya"),
    ),
]

LARGE_THREAD = ThreadScenario(
    channel_slug="support",
    title="Support transcript backfill load test",
    body=(
        "Long-running support thread used to prove large histories, load-more behavior, search, labels, and saved source links. "
        "This intentionally has hundreds of realistic updates. $support $performance $backfill"
    ),
    creator="sam",
    days_ago=44,
    comments=(
        CommentSeed("sam", "Starting the import replay transcript. I will keep each checkpoint short and searchable. $backfill", 18),
        CommentSeed("ellis", "Baseline: 18,400 messages, 126 channels, 0 private-channel leaks in the dry run.", 37, reactions=(("ivy", "✅"),)),
        CommentSeed("rhea", "Customer asks that we preserve attribution where legacy records include operator initials only.", 59),
    ),
    reactions=(("ellis", "👀"), ("anchor", "✅")),
    saved_by=("anchor", "sam"),
    generated_comments=420,
    generator="backfill",
)


def main() -> None:
    parser = argparse.ArgumentParser(description="Populate a local sshoosh SQLite database with rich demo data.")
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
        f"DM messages: {summary['dm_messages']}. "
        f"Notifications: {summary['notifications']}. "
        f"Reactions: {summary['reactions']}. "
        f"Saved items: {summary['saved_messages']}. "
        f"Labels: {summary['labels']}."
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
        if version == "20260501000001_notification_archive" and column_exists(conn, "notifications", "archived_at"):
            conn.execute(
                """
                CREATE INDEX IF NOT EXISTS idx_notifications_account_archived
                  ON notifications(account_id, archived_at, created_at DESC)
                """
            )
            conn.execute(
                "INSERT INTO _sshoosh_migrations (version, applied_at) VALUES (?, ?)",
                (version, ts(datetime.now(timezone.utc))),
            )
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
        "search_documents",
        "message_labels",
        "saved_messages",
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
    delete_where_if_exists(conn, "saved_messages", "account_id LIKE 'demo-account-%' OR source_id LIKE 'demo-%'")
    delete_where_if_exists(conn, "message_labels", "source_id LIKE 'demo-%'")
    delete_where_if_exists(conn, "search_documents", "object_id LIKE 'demo-%'")
    delete_where_if_exists(conn, "search_index", "object_id LIKE 'demo-%'")
    for table in ["notifications", "reactions", "mentions", "audit_log", "presence_sessions"]:
        delete_where_if_exists(conn, table, "id LIKE 'demo-%'")
    conn.execute("DELETE FROM event_log WHERE kind LIKE 'demo.%'")
    conn.execute("DELETE FROM conversations WHERE id LIKE 'demo-conversation-%'")
    conn.execute("DELETE FROM threads WHERE id LIKE 'demo-thread-%'")
    conn.execute("DELETE FROM channels WHERE id LIKE 'demo-channel-%' AND slug <> 'general'")
    conn.execute("DELETE FROM accounts WHERE id LIKE 'demo-account-%'")
    conn.execute("DELETE FROM search_index")
    delete_if_exists(conn, "search_documents")


def delete_if_exists(conn: sqlite3.Connection, table: str) -> None:
    exists = conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type IN ('table', 'view') AND name = ?",
        (table,),
    ).fetchone()
    if exists:
        conn.execute(f"DELETE FROM {table}")


def delete_where_if_exists(conn: sqlite3.Connection, table: str, where: str) -> None:
    exists = conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type IN ('table', 'view') AND name = ?",
        (table,),
    ).fetchone()
    if exists:
        conn.execute(f"DELETE FROM {table} WHERE {where}")


def column_exists(conn: sqlite3.Connection, table: str, column: str) -> bool:
    return any(row["name"] == column for row in conn.execute(f"PRAGMA table_info({table})"))


def seed_demo_data(conn: sqlite3.Connection, anchor: Account) -> dict[str, int]:
    now = datetime.now(timezone.utc)
    start = now - timedelta(days=182)
    accounts = seed_accounts(conn, anchor, now, start)
    channel_ids = seed_channels(conn, accounts, anchor, now, start)

    summary = {
        "accounts": len({account.id for account in accounts.values()}),
        "channels": len(CHANNELS),
        "threads": 0,
        "comments": 0,
        "conversations": 0,
        "dm_messages": 0,
        "mentions": 0,
        "notifications": 0,
        "reactions": 0,
        "saved_messages": 0,
        "labels": 0,
    }
    counters = {
        "thread": 0,
        "comment": 0,
        "mention": 0,
        "notification": 0,
        "reaction": 0,
        "conversation": 0,
        "dm": 0,
    }

    used_title_keys: dict[str, set[str]] = {}
    for scenario in SHOWCASE_THREADS:
        seed_thread(conn, accounts, channel_ids, anchor, now, counters, summary, used_title_keys, scenario)
    seed_thread(conn, accounts, channel_ids, anchor, now, counters, summary, used_title_keys, LARGE_THREAD)
    for scenario in weekly_scenarios():
        seed_thread(conn, accounts, channel_ids, anchor, now, counters, summary, used_title_keys, scenario)

    for scenario in DM_SCENARIOS:
        seed_dm(conn, accounts, anchor, now, counters, summary, scenario)

    seed_presence(conn, accounts, now)
    seed_admin_artifacts(conn, accounts, anchor, now, summary)
    rebuild_search_index(conn)
    summary["accounts"] = conn.execute("SELECT COUNT(*) AS count FROM accounts").fetchone()["count"]
    summary["labels"] = conn.execute("SELECT COUNT(*) AS count FROM message_labels").fetchone()["count"]
    summary["saved_messages"] = conn.execute("SELECT COUNT(*) AS count FROM saved_messages").fetchone()["count"]
    return summary


def seed_accounts(
    conn: sqlite3.Connection,
    anchor: Account,
    now: datetime,
    start: datetime,
) -> dict[str, Account]:
    accounts = {"anchor": anchor, anchor.username: anchor}
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
        if person.username == anchor.username:
            accounts[person.username] = anchor
            continue

        existing = conn.execute(
            """
            SELECT id, username, display_name
            FROM accounts
            WHERE username = ? AND id <> ?
            """,
            (person.username, person.id),
        ).fetchone()
        if existing:
            accounts[person.username] = Account(existing["id"], existing["username"], existing["display_name"])
            continue

        created_at = start + timedelta(days=2 + index * 3)
        seen_at = now - timedelta(minutes=12 + index * 17)
        settings = {
            "profile": {
                "title": person.title,
                "team": person.team,
                "location": person.location,
            },
            "terminal_notifications": person.username in {"sam", "zoe", "ivy"},
        }
        conn.execute(
            """
            INSERT INTO accounts
              (id, username, display_name, role, settings_json, created_at, updated_at, last_seen_at, activated_at, disabled_at, pending_username)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL)
            ON CONFLICT(id) DO UPDATE SET
              username = excluded.username,
              display_name = excluded.display_name,
              role = excluded.role,
              settings_json = excluded.settings_json,
              updated_at = excluded.updated_at,
              last_seen_at = excluded.last_seen_at,
              activated_at = excluded.activated_at,
              disabled_at = NULL,
              pending_username = NULL
            """,
            (
                person.id,
                person.username,
                person.display_name,
                person.role,
                json.dumps(settings),
                ts(created_at),
                ts(seen_at),
                ts(seen_at),
                ts(created_at),
            ),
        )
        accounts[person.username] = Account(person.id, person.username, person.display_name)

    return accounts


def seed_channels(
    conn: sqlite3.Connection,
    accounts: dict[str, Account],
    anchor: Account,
    now: datetime,
    start: datetime,
) -> dict[str, str]:
    channel_ids: dict[str, str] = {}
    for index, channel in enumerate(CHANNELS):
        created_at = start + timedelta(days=index)
        updated_at = now - timedelta(days=max(1, len(CHANNELS) - index))
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
        channel_ids[channel.slug] = channel_id
        conn.execute("DELETE FROM channel_members WHERE channel_id = ?", (channel_id,))
        for username in channel.members:
            account = accounts[username]
            role = "admin" if username in {"anchor", "marco"} else "member"
            conn.execute(
                "INSERT INTO channel_members (channel_id, account_id, role, joined_at) VALUES (?, ?, ?, ?)",
                (channel_id, account.id, role, ts(created_at + timedelta(hours=len(username)))),
            )

    return channel_ids


def seed_thread(
    conn: sqlite3.Connection,
    accounts: dict[str, Account],
    channel_ids: dict[str, str],
    anchor: Account,
    now: datetime,
    counters: dict[str, int],
    summary: dict[str, int],
    used_title_keys: dict[str, set[str]],
    scenario: ThreadScenario,
) -> str:
    channel = channel_by_slug(scenario.channel_slug)
    channel_id = channel_ids[channel.slug]
    members = [accounts[username] for username in channel.members]
    creator = accounts[scenario.creator]
    created_at = now - timedelta(days=scenario.days_ago, hours=2 + counters["thread"] % 8)
    thread_id = next_demo_id(counters, "thread", width=4)
    title = render_text(scenario.title, anchor)
    title_key = normalize_title_key(title)
    channel_keys = used_title_keys.setdefault(channel_id, set())
    if title_key in channel_keys:
        title = f"{title} {counters['thread']}"
        title_key = normalize_title_key(title)
    channel_keys.add(title_key)
    body = render_text(scenario.body, anchor)
    archived_at = ts(created_at + timedelta(days=3)) if scenario.archived else None
    pinned_at = ts(created_at + timedelta(minutes=25)) if scenario.pinned else None
    edited_at = ts(created_at + timedelta(hours=2)) if scenario.edited else None

    conn.execute(
        """
        INSERT INTO threads
          (id, channel_id, creator_account_id, title, name_key, body, comment_count, last_comment_index,
           last_activity_at, created_at, updated_at, edited_at, archived_at, pinned_at, deleted_at)
        VALUES (?, ?, ?, ?, ?, ?, 0, 0, ?, ?, ?, ?, ?, ?, NULL)
        """,
        (
            thread_id,
            channel_id,
            creator.id,
            title,
            title_key,
            body,
            ts(created_at),
            ts(created_at),
            ts(created_at),
            edited_at,
            archived_at,
            pinned_at,
        ),
    )
    summary["threads"] += 1
    insert_labels(conn, summary, "thread", thread_id, channel_id, thread_id, None, None, f"{title}\n{body}", ts(created_at))
    create_mentions_for_body(
        conn,
        accounts,
        set(channel.members),
        anchor,
        counters,
        summary,
        body,
        creator,
        created_at,
        "thread",
        thread_id,
        channel_id,
        thread_id,
        None,
        None,
        title,
    )
    insert_event(
        conn,
        created_at,
        channel_id,
        thread_id,
        None,
        "demo.thread.created",
        {"title": title, "channel": channel.slug, "creator": creator.username},
    )
    for reactor_name, emoji in scenario.reactions:
        insert_reaction(conn, counters, summary, "thread", thread_id, accounts[reactor_name], emoji, created_at + timedelta(minutes=40))

    comments = list(scenario.comments)
    if scenario.generated_comments:
        comments.extend(generated_comments(scenario, channel, scenario.generated_comments))

    last_activity = created_at
    comment_ids_by_index: dict[int, str] = {}
    for comment_index, comment in enumerate(comments, start=1):
        author = accounts[comment.author]
        comment_at = created_at + timedelta(minutes=comment.minutes_after)
        last_activity = max(last_activity, comment_at)
        comment_id = next_demo_id(counters, "comment", width=5)
        comment_body = render_text(comment.body, anchor)
        edited_at = ts(comment_at + timedelta(minutes=9)) if comment.edited else None
        conn.execute(
            """
            INSERT INTO comments
              (id, thread_id, channel_id, author_account_id, obj_index, body, created_at, updated_at, edited_at, deleted_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)
            """,
            (comment_id, thread_id, channel_id, author.id, comment_index, comment_body, ts(comment_at), ts(comment_at), edited_at),
        )
        summary["comments"] += 1
        comment_ids_by_index[comment_index] = comment_id
        insert_labels(conn, summary, "comment", comment_id, channel_id, thread_id, None, comment_index, comment_body, ts(comment_at))
        create_mentions_for_body(
            conn,
            accounts,
            set(channel.members),
            anchor,
            counters,
            summary,
            comment_body,
            author,
            comment_at,
            "comment",
            comment_id,
            channel_id,
            thread_id,
            None,
            comment_index,
            title,
        )
        if comment_index in {len(comments), 2} and creator.id != author.id:
            create_notification(
                conn,
                counters,
                summary,
                creator,
                author,
                "reply",
                "comment",
                comment_id,
                channel_id,
                thread_id,
                None,
                f"{author.display_name} replied in #{channel.slug}",
                comment_body,
                comment_at,
                anchor,
            )
        for reactor_name, emoji in comment.reactions:
            insert_reaction(conn, counters, summary, "comment", comment_id, accounts[reactor_name], emoji, comment_at + timedelta(minutes=8))
        for account_name in comment.saved_by:
            insert_saved_message(conn, summary, accounts[account_name], "comment", comment_id, comment_at + timedelta(minutes=17))

    comment_count = len(comments)
    conn.execute(
        """
        UPDATE threads
        SET comment_count = ?, last_comment_index = ?, last_activity_at = ?, updated_at = ?
        WHERE id = ?
        """,
        (comment_count, comment_count, ts(last_activity), ts(last_activity), thread_id),
    )

    explicit_unreads = dict(scenario.unread_by)
    for account in members:
        explicit = explicit_unread_for(account, scenario.unread_by, anchor)
        if explicit is not None:
            unread_count = min(max(explicit, 0), comment_count)
        elif account.id == creator.id or scenario.archived:
            unread_count = 0
        elif scenario.days_ago <= 14 and (len(account.username) + counters["thread"]) % 5 == 0:
            unread_count = min(2 + len(account.username) % 3, comment_count)
        else:
            unread_count = 0
        last_read = max(0, comment_count - unread_count)
        muted_until = ts(now + timedelta(days=5)) if account_matches(account, scenario.muted_for, anchor) else None
        saved_at = ts(created_at + timedelta(hours=3)) if account_matches(account, scenario.saved_by, anchor) else None
        conn.execute(
            """
            INSERT INTO thread_reads
              (thread_id, account_id, last_read_index, unread_count, marked_unread_at, muted_until, saved_at)
            VALUES (?, ?, ?, ?, NULL, ?, ?)
            """,
            (thread_id, account.id, last_read, unread_count, muted_until, saved_at),
        )

    for account_name in scenario.saved_by:
        if comment_ids_by_index:
            last_comment_id = comment_ids_by_index[max(comment_ids_by_index)]
            insert_saved_message(conn, summary, accounts[account_name], "comment", last_comment_id, last_activity + timedelta(minutes=21))

    return thread_id


def seed_dm(
    conn: sqlite3.Connection,
    accounts: dict[str, Account],
    anchor: Account,
    now: datetime,
    counters: dict[str, int],
    summary: dict[str, int],
    scenario: DmScenario,
) -> None:
    left = accounts[scenario.left]
    right = accounts[scenario.right]
    participants = {left.username, right.username}
    created_at = now - timedelta(days=scenario.days_ago, hours=1 + counters["conversation"] % 5)
    conversation_id = next_demo_id(counters, "conversation", width=4)
    message_count = len(scenario.messages)
    last_activity = created_at + timedelta(minutes=scenario.messages[-1].minutes_after)
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
            ts(last_activity),
            ts(created_at),
        ),
    )
    summary["conversations"] += 1

    message_ids: list[str] = []
    for message_index, message in enumerate(scenario.messages, start=1):
        author = accounts[message.author]
        message_at = created_at + timedelta(minutes=message.minutes_after)
        message_id = next_demo_id(counters, "dm", width=5)
        body = render_text(message.body, anchor)
        edited_at = ts(message_at + timedelta(minutes=11)) if message.edited else None
        conn.execute(
            """
            INSERT INTO conversation_messages
              (id, conversation_id, author_account_id, obj_index, body, created_at, updated_at, edited_at, deleted_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL)
            """,
            (message_id, conversation_id, author.id, message_index, body, ts(message_at), ts(message_at), edited_at),
        )
        summary["dm_messages"] += 1
        message_ids.append(message_id)
        insert_labels(conn, summary, "dm", message_id, None, None, conversation_id, message_index, body, ts(message_at))
        create_mentions_for_body(
            conn,
            accounts,
            participants,
            anchor,
            counters,
            summary,
            body,
            author,
            message_at,
            "dm",
            message_id,
            None,
            None,
            conversation_id,
            message_index,
            f"DM @{left.username} @{right.username}",
        )
        for target in [left, right]:
            if target.id == author.id:
                continue
            create_notification(
                conn,
                counters,
                summary,
                target,
                author,
                "dm",
                "dm",
                message_id,
                None,
                None,
                conversation_id,
                f"New DM from {author.display_name}",
                body,
                message_at,
                anchor,
            )
        for reactor_name, emoji in message.reactions:
            insert_reaction(conn, counters, summary, "dm", message_id, accounts[reactor_name], emoji, message_at + timedelta(minutes=7))
        for account_name in message.saved_by:
            insert_saved_message(conn, summary, accounts[account_name], "dm", message_id, message_at + timedelta(minutes=12))

    for member in [left, right]:
        explicit = explicit_unread_for(member, scenario.unread_by, anchor)
        if explicit is not None:
            unread_count = min(max(explicit, 0), message_count)
        elif member.username == scenario.right and scenario.days_ago <= 7:
            unread_count = 1 if scenario.messages[-1].author != member.username else 0
        else:
            unread_count = 0
        last_read = max(0, message_count - unread_count)
        muted_until = ts(now + timedelta(days=4)) if account_matches(member, scenario.muted_by, anchor) else None
        saved_at = ts(created_at + timedelta(hours=4)) if account_matches(member, scenario.saved_by, anchor) else None
        conn.execute(
            """
            INSERT INTO conversation_members
              (conversation_id, account_id, joined_at, last_read_index, unread_count, muted_until, saved_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            """,
            (conversation_id, member.id, ts(created_at), last_read, unread_count, muted_until, saved_at),
        )

    if message_ids:
        for account_name in scenario.saved_by:
            insert_saved_message(conn, summary, accounts[account_name], "dm", message_ids[-1], last_activity + timedelta(minutes=18))

    insert_event(
        conn,
        created_at,
        None,
        None,
        conversation_id,
        "demo.conversation.created",
        {"participants": [left.username, right.username]},
    )


def weekly_scenarios() -> list[ThreadScenario]:
    scenarios: list[ThreadScenario] = []
    for week in range(1, 25):
        template_count = 2 if week % 3 == 0 else 1
        for lane in range(template_count):
            template = WEEKLY_TEMPLATES[(week + lane - 1) % len(WEEKLY_TEMPLATES)]
            channel_slug, title, body, creator, authors, snippets = template
            comments = []
            for index, snippet in enumerate(snippets):
                comments.append(
                    CommentSeed(
                        authors[index % len(authors)],
                        f"{snippet} Week {week} note {index + 1}.",
                        45 + index * 72 + lane * 18,
                        reactions=((authors[(index + 1) % len(authors)], "✅"),) if index == 1 else (),
                    )
                )
            days_ago = 182 - week * 7 + lane
            scenarios.append(
                ThreadScenario(
                    channel_slug=channel_slug,
                    title=title.format(week=week),
                    body=body,
                    creator=creator,
                    days_ago=max(days_ago, 20),
                    comments=tuple(comments),
                    reactions=((authors[1], "👀"),),
                    saved_by=("anchor",) if week % 6 == 0 else (),
                    muted_for=("anchor",) if week % 11 == 0 else (),
                    unread_by=(("anchor", 1),) if week % 7 == 0 else (),
                    archived=week < 5 and channel_slug in {"ops", "support"},
                )
            )
    return scenarios


def generated_comments(scenario: ThreadScenario, channel: Channel, count: int) -> list[CommentSeed]:
    authors = [username for username in channel.members if username != "anchor"] or ["anchor"]
    templates = [
        "Checkpoint {idx}: replay window advanced cleanly; source rows and indexed rows still match. $backfill",
        "Checkpoint {idx}: customer-facing status remains green; no duplicate source identities found. $support",
        "Checkpoint {idx}: private-channel sample passed visibility checks for members and non-members. $security",
        "Checkpoint {idx}: export checksum matches the previous dry run and attachments are still streaming. $customer-acme",
        "Checkpoint {idx}: search query for key rotation returns the current incident before older docs. $search",
    ]
    comments: list[CommentSeed] = []
    for idx in range(1, count + 1):
        body = templates[idx % len(templates)].format(idx=idx)
        if idx % 37 == 0:
            body += " @{anchor} this is a good source link for the live demo."
        if idx % 53 == 0:
            body += " @sam please keep this in the support macro."
        reactions: tuple[tuple[str, str], ...] = ()
        if idx % 8 == 0:
            reactions = ((authors[(idx + 1) % len(authors)], "✅"),)
        saved_by = ("anchor",) if idx in {42, 180, 360} else ()
        comments.append(
            CommentSeed(
                authors[idx % len(authors)],
                body,
                90 + idx * 11,
                edited=idx % 89 == 0,
                reactions=reactions,
                saved_by=saved_by,
            )
        )
    return comments


def seed_presence(conn: sqlite3.Connection, accounts: dict[str, Account], now: datetime) -> None:
    active = ["anchor", "marco", "sam", "nina", "ivy", "omar", "zoe"]
    for index, username in enumerate(active):
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
                ts(now - timedelta(hours=1, minutes=index * 6)),
                ts(now - timedelta(minutes=2 + index * 3)),
                "demo-node-a" if index % 2 == 0 else "demo-node-b",
            ),
        )
    for username in ["priya", "jules", "luca", "maya"]:
        account = accounts[username]
        disconnected_at = now - timedelta(minutes=25 + len(username) * 4)
        conn.execute(
            """
            INSERT INTO presence_sessions
              (id, account_id, started_at, last_seen_at, disconnected_at, node_id)
            VALUES (?, ?, ?, ?, ?, ?)
            """,
            (
                f"demo-presence-away-{account.username}",
                account.id,
                ts(disconnected_at - timedelta(hours=2)),
                ts(disconnected_at),
                ts(disconnected_at),
                "demo-node-a",
            ),
        )


def seed_admin_artifacts(
    conn: sqlite3.Connection,
    accounts: dict[str, Account],
    anchor: Account,
    now: datetime,
    summary: dict[str, int],
) -> None:
    invite_rows = [
        ("demo-invite-support", "demo-hash-support", "member", "sam", None, now - timedelta(days=3), now + timedelta(days=4)),
        ("demo-invite-admin", "demo-hash-admin", "admin", "anchor", None, now - timedelta(days=1), now + timedelta(days=2)),
    ]
    for invite_id, code_hash, role, creator_name, accepted_by, created_at, expires_at in invite_rows:
        creator = accounts[creator_name]
        accepted = accounts[accepted_by].id if accepted_by else None
        conn.execute(
            """
            INSERT INTO invites
              (id, code_hash, role_on_accept, created_by_account_id, accepted_by_account_id, created_at, expires_at, revoked_at, accepted_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, NULL, NULL)
            """,
            (invite_id, code_hash, role, creator.id, accepted, ts(created_at), ts(expires_at)),
        )

    audit_rows = [
        ("demo-audit-seed", anchor.id, "demo.seeded", "workspace", {"anchor_username": anchor.username, **summary}, now),
        ("demo-audit-security", accounts["ivy"].id, "security.reviewed", "keyboard-interactive tokens", {"result": "accepted", "labels": ["security", "tokens"]}, now - timedelta(days=6)),
        ("demo-audit-invite", accounts["sam"].id, "invite.created", "support rotation", {"role": "member", "expires_in_days": 4}, now - timedelta(days=3)),
        ("demo-audit-export", accounts["maya"].id, "export.created", "acme incident packet", {"format": "json", "source": "support thread"}, now - timedelta(days=2)),
    ]
    for audit_id, actor_id, action, target, metadata, created_at in audit_rows:
        conn.execute(
            """
            INSERT INTO audit_log
              (id, actor_account_id, action, target, metadata_json, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            """,
            (audit_id, actor_id, action, target, json.dumps(metadata), ts(created_at)),
        )


def insert_labels(
    conn: sqlite3.Connection,
    summary: dict[str, int],
    source_kind: str,
    source_id: str,
    channel_id: str | None,
    thread_id: str | None,
    conversation_id: str | None,
    obj_index: int | None,
    text: str,
    created_at: str,
) -> None:
    for tag in parse_labels(text):
        conn.execute(
            """
            INSERT OR IGNORE INTO message_labels
              (tag, source_kind, source_id, channel_id, thread_id, conversation_id, obj_index, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (tag, source_kind, source_id, channel_id, thread_id, conversation_id, obj_index, created_at),
        )
        summary["labels"] += 1


def create_mentions_for_body(
    conn: sqlite3.Connection,
    accounts: dict[str, Account],
    visible_usernames: set[str],
    anchor: Account,
    counters: dict[str, int],
    summary: dict[str, int],
    body: str,
    actor: Account,
    created_at: datetime,
    source_kind: str,
    source_id: str,
    channel_id: str | None,
    thread_id: str | None,
    conversation_id: str | None,
    obj_index: int | None,
    source_title: str,
) -> None:
    seen: set[str] = set()
    for username in MENTION_RE.findall(body):
        if username in seen or username not in visible_usernames:
            continue
        seen.add(username)
        target = accounts.get(username)
        if target is None or target.id == actor.id:
            continue
        mention_id = next_demo_id(counters, "mention", width=5)
        read_at = read_at_for(target, anchor, created_at)
        conn.execute(
            """
            INSERT INTO mentions
              (id, target_account_id, actor_account_id, source_kind, source_id, channel_id, thread_id,
               conversation_id, obj_index, created_at, read_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                mention_id,
                target.id,
                actor.id,
                source_kind,
                source_id,
                channel_id,
                thread_id,
                conversation_id,
                obj_index,
                ts(created_at),
                read_at,
            ),
        )
        summary["mentions"] += 1
        kind_label = "DM" if source_kind == "dm" else source_title
        create_notification(
            conn,
            counters,
            summary,
            target,
            actor,
            "mention",
            source_kind,
            source_id,
            channel_id,
            thread_id,
            conversation_id,
            f"{actor.display_name} mentioned you",
            f"{kind_label}: {body}",
            created_at,
            anchor,
            read_at,
        )


def create_notification(
    conn: sqlite3.Connection,
    counters: dict[str, int],
    summary: dict[str, int],
    target: Account,
    actor: Account,
    kind: str,
    source_kind: str,
    source_id: str,
    channel_id: str | None,
    thread_id: str | None,
    conversation_id: str | None,
    title: str,
    body: str,
    created_at: datetime,
    anchor: Account,
    read_at: str | None = None,
) -> None:
    if read_at is None:
        read_at = read_at_for(target, anchor, created_at)
    notification_id = next_demo_id(counters, "notification", width=5)
    conn.execute(
        """
        INSERT INTO notifications
          (id, account_id, actor_account_id, kind, source_kind, source_id, channel_id, thread_id,
           conversation_id, title, body, created_at, read_at, archived_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)
        """,
        (
            notification_id,
            target.id,
            actor.id,
            kind,
            source_kind,
            source_id,
            channel_id,
            thread_id,
            conversation_id,
            title,
            body,
            ts(created_at),
            read_at,
        ),
    )
    summary["notifications"] += 1


def insert_reaction(
    conn: sqlite3.Connection,
    counters: dict[str, int],
    summary: dict[str, int],
    source_kind: str,
    source_id: str,
    account: Account,
    emoji: str,
    created_at: datetime,
) -> None:
    conn.execute(
        """
        INSERT OR IGNORE INTO reactions
          (id, source_kind, source_id, account_id, emoji, created_at)
        VALUES (?, ?, ?, ?, ?, ?)
        """,
        (next_demo_id(counters, "reaction", width=5), source_kind, source_id, account.id, emoji, ts(created_at)),
    )
    summary["reactions"] += 1


def insert_saved_message(
    conn: sqlite3.Connection,
    summary: dict[str, int],
    account: Account,
    source_kind: str,
    source_id: str,
    saved_at: datetime,
) -> None:
    conn.execute(
        """
        INSERT OR IGNORE INTO saved_messages
          (account_id, source_kind, source_id, saved_at)
        VALUES (?, ?, ?, ?)
        """,
        (account.id, source_kind, source_id, ts(saved_at)),
    )
    summary["saved_messages"] += 1


def rebuild_search_index(conn: sqlite3.Connection) -> None:
    conn.execute("DELETE FROM search_index")
    delete_if_exists(conn, "search_documents")
    index_rows = []
    index_rows.extend(
        conn.execute(
            """
            SELECT 'thread' AS kind, t.id AS object_id, t.channel_id, t.id AS thread_id, NULL AS conversation_id,
                   t.title, t.body, '#' || c.slug AS context
            FROM threads t
            JOIN channels c ON c.id = t.channel_id
            WHERE t.deleted_at IS NULL
            """
        ).fetchall()
    )
    index_rows.extend(
        conn.execute(
            """
            SELECT 'comment' AS kind, cm.id AS object_id, cm.channel_id, cm.thread_id, NULL AS conversation_id,
                   t.title, cm.body, '#' || c.slug AS context
            FROM comments cm
            JOIN threads t ON t.id = cm.thread_id
            JOIN channels c ON c.id = cm.channel_id
            WHERE cm.deleted_at IS NULL AND t.deleted_at IS NULL
            """
        ).fetchall()
    )
    index_rows.extend(
        conn.execute(
            """
            SELECT 'dm' AS kind, m.id AS object_id, NULL AS channel_id, NULL AS thread_id, m.conversation_id,
                   'DM' AS title, m.body, 'DM' AS context
            FROM conversation_messages m
            WHERE m.deleted_at IS NULL
            """
        ).fetchall()
    )
    for row in index_rows:
        result = conn.execute(
            """
            INSERT INTO search_documents
              (kind, object_id, channel_id, thread_id, conversation_id)
            VALUES (?, ?, ?, ?, ?)
            """,
            (row["kind"], row["object_id"], row["channel_id"], row["thread_id"], row["conversation_id"]),
        )
        conn.execute(
            """
            INSERT INTO search_index
              (rowid, kind, object_id, channel_id, thread_id, conversation_id, title, body, context)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                result.lastrowid,
                row["kind"],
                row["object_id"],
                row["channel_id"],
                row["thread_id"],
                row["conversation_id"],
                row["title"],
                row["body"],
                row["context"],
            ),
        )


def parse_labels(text: str) -> list[str]:
    tags: list[str] = []
    for raw in LABEL_RE.findall(text):
        tag = normalize_label(raw)
        if tag and tag not in tags:
            tags.append(tag)
    return tags


def normalize_label(value: str) -> str | None:
    tag = value.strip().removeprefix("$")
    if not tag:
        return None
    if not any(ch.isalpha() for ch in tag):
        return None
    if not all(ch.isascii() and (ch.isalnum() or ch in "_-") for ch in tag):
        return None
    return tag.lower()


def read_at_for(target: Account, anchor: Account, created_at: datetime) -> str | None:
    age = datetime.now(timezone.utc) - created_at
    if target.id == anchor.id:
        return None if age.days <= 10 else ts(created_at + timedelta(hours=5))
    if (len(target.username) + created_at.day) % 4 == 0:
        return None
    return ts(created_at + timedelta(hours=3))


def account_matches(account: Account, names: tuple[str, ...], anchor: Account) -> bool:
    return account.username in names or ("anchor" in names and account.id == anchor.id)


def explicit_unread_for(account: Account, unread_by: tuple[tuple[str, int], ...], anchor: Account) -> int | None:
    for username, count in unread_by:
        if username == account.username or (username == "anchor" and account.id == anchor.id):
            return count
    return None


def insert_event(
    conn: sqlite3.Connection,
    created_at: datetime,
    channel_id: str | None,
    thread_id: str | None,
    conversation_id: str | None,
    kind: str,
    payload: dict[str, object],
) -> None:
    conn.execute(
        """
        INSERT INTO event_log
          (created_at, channel_id, thread_id, conversation_id, kind, payload_json)
        VALUES (?, ?, ?, ?, ?, ?)
        """,
        (ts(created_at), channel_id, thread_id, conversation_id, kind, json.dumps(payload)),
    )


def channel_by_slug(slug: str) -> Channel:
    for channel in CHANNELS:
        if channel.slug == slug:
            return channel
    raise RuntimeError(f"missing channel #{slug}")


def get_channel_id(conn: sqlite3.Connection, slug: str) -> str:
    row = conn.execute("SELECT id FROM channels WHERE slug = ?", (slug,)).fetchone()
    if row is None:
        raise RuntimeError(f"missing channel #{slug}")
    return row["id"]


def normalize_title_key(value: str) -> str:
    out = ""
    for char in value.strip().lower():
        if char.isascii() and char.isalnum():
            out += char
        elif char in "-_. " and not out.endswith("-"):
            out += "-"
    return out.strip("-")


def render_text(value: str, anchor: Account) -> str:
    return value.format(anchor=anchor.username)


def next_demo_id(counters: dict[str, int], kind: str, width: int) -> str:
    value = counters[kind]
    counters[kind] += 1
    return f"demo-{kind}-{value:0{width}d}"


def dm_key(left: str, right: str) -> str:
    return f"{left}:{right}" if left <= right else f"{right}:{left}"


def ts(value: datetime) -> str:
    return value.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")


if __name__ == "__main__":
    main()
