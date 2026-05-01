use super::*;
pub(crate) const INVITE_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "new",
        aliases: &["create"],
        description: "Create an invite code",
        args: "[member|admin] [hours]",
    },
    SubcommandSpec {
        name: "list",
        aliases: &["ls"],
        description: "List invites",
        args: "",
    },
    SubcommandSpec {
        name: "revoke",
        aliases: &["remove"],
        description: "Revoke an invite",
        args: "invite-id",
    },
];

pub(crate) const CHANNEL_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "new",
        aliases: &["create"],
        description: "Create a public channel",
        args: "name",
    },
    SubcommandSpec {
        name: "private",
        aliases: &[],
        description: "Create a private channel",
        args: "name",
    },
    SubcommandSpec {
        name: "list",
        aliases: &["ls"],
        description: "List joined and joinable channels",
        args: "",
    },
    SubcommandSpec {
        name: "join",
        aliases: &[],
        description: "Join a public channel",
        args: "#channel",
    },
    SubcommandSpec {
        name: "leave",
        aliases: &[],
        description: "Leave current or named channel",
        args: "[#channel]",
    },
    SubcommandSpec {
        name: "rename",
        aliases: &[],
        description: "Rename a channel",
        args: "[#channel] name",
    },
    SubcommandSpec {
        name: "topic",
        aliases: &[],
        description: "Set a channel topic",
        args: "[#channel] topic",
    },
    SubcommandSpec {
        name: "archive",
        aliases: &[],
        description: "Archive current or named channel",
        args: "[#channel]",
    },
    SubcommandSpec {
        name: "unarchive",
        aliases: &[],
        description: "Unarchive a channel",
        args: "#channel",
    },
    SubcommandSpec {
        name: "members",
        aliases: &[],
        description: "List private channel members",
        args: "#channel",
    },
    SubcommandSpec {
        name: "add",
        aliases: &["add-member"],
        description: "Add a private channel member",
        args: "#channel @user",
    },
    SubcommandSpec {
        name: "remove",
        aliases: &["remove-member"],
        description: "Remove a private channel member",
        args: "#channel @user",
    },
];

pub(crate) const THREAD_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "new",
        aliases: &["create"],
        description: "Create a thread",
        args: "title",
    },
    SubcommandSpec {
        name: "rename",
        aliases: &["edit"],
        description: "Rename current thread",
        args: "title",
    },
    SubcommandSpec {
        name: "delete",
        aliases: &["remove"],
        description: "Delete current thread",
        args: "",
    },
    SubcommandSpec {
        name: "archive",
        aliases: &[],
        description: "Archive current thread",
        args: "",
    },
    SubcommandSpec {
        name: "unarchive",
        aliases: &[],
        description: "Unarchive current thread",
        args: "",
    },
    SubcommandSpec {
        name: "pin",
        aliases: &[],
        description: "Pin current thread",
        args: "",
    },
    SubcommandSpec {
        name: "unpin",
        aliases: &[],
        description: "Unpin current thread",
        args: "",
    },
    SubcommandSpec {
        name: "mute",
        aliases: &[],
        description: "Mute current thread",
        args: "[hours]",
    },
    SubcommandSpec {
        name: "unmute",
        aliases: &[],
        description: "Unmute current thread",
        args: "",
    },
    SubcommandSpec {
        name: "read",
        aliases: &[],
        description: "Mark current thread read",
        args: "",
    },
    SubcommandSpec {
        name: "unread",
        aliases: &[],
        description: "Mark current thread unread",
        args: "",
    },
];

pub(crate) const DM_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "open",
        aliases: &[],
        description: "Open a direct message",
        args: "@user",
    },
    SubcommandSpec {
        name: "edit",
        aliases: &[],
        description: "Edit a DM message",
        args: "index body",
    },
    SubcommandSpec {
        name: "delete",
        aliases: &["remove"],
        description: "Delete a DM message",
        args: "index",
    },
    SubcommandSpec {
        name: "mute",
        aliases: &[],
        description: "Mute current DM",
        args: "[hours]",
    },
    SubcommandSpec {
        name: "unmute",
        aliases: &[],
        description: "Unmute current DM",
        args: "",
    },
    SubcommandSpec {
        name: "read",
        aliases: &[],
        description: "Mark current DM read",
        args: "",
    },
    SubcommandSpec {
        name: "unread",
        aliases: &[],
        description: "Mark current DM unread",
        args: "",
    },
];

pub(crate) const USER_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "list",
        aliases: &["ls"],
        description: "List users",
        args: "",
    },
    SubcommandSpec {
        name: "profile",
        aliases: &[],
        description: "Update your display name",
        args: "display-name",
    },
    SubcommandSpec {
        name: "username",
        aliases: &[],
        description: "Update your username",
        args: "username",
    },
    SubcommandSpec {
        name: "disable",
        aliases: &[],
        description: "Disable a user",
        args: "@user",
    },
    SubcommandSpec {
        name: "enable",
        aliases: &[],
        description: "Enable a user",
        args: "@user",
    },
    SubcommandSpec {
        name: "role",
        aliases: &[],
        description: "Set a user role",
        args: "@user owner|admin|member",
    },
];

pub(crate) const KEY_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "list",
        aliases: &["ls"],
        description: "List SSH keys",
        args: "",
    },
    SubcommandSpec {
        name: "my",
        aliases: &["mine"],
        description: "List your SSH keys",
        args: "",
    },
    SubcommandSpec {
        name: "add",
        aliases: &[],
        description: "Add an SSH public key",
        args: "public-key [| label]",
    },
    SubcommandSpec {
        name: "label",
        aliases: &[],
        description: "Rename an SSH key label",
        args: "key label",
    },
    SubcommandSpec {
        name: "revoke",
        aliases: &["remove"],
        description: "Revoke an SSH key",
        args: "key-id|fingerprint",
    },
];

pub(crate) const COMMENT_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "edit",
        aliases: &[],
        description: "Edit a thread comment",
        args: "index body",
    },
    SubcommandSpec {
        name: "delete",
        aliases: &["remove"],
        description: "Delete a thread comment",
        args: "index",
    },
];

pub(crate) const NOTIFICATION_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "list",
        aliases: &["ls"],
        description: "List notifications",
        args: "",
    },
    SubcommandSpec {
        name: "mentions",
        aliases: &[],
        description: "List your mentions",
        args: "",
    },
    SubcommandSpec {
        name: "read",
        aliases: &["mark-read"],
        description: "Mark one or all notifications read",
        args: "[notification-id]",
    },
    SubcommandSpec {
        name: "terminal",
        aliases: &[],
        description: "Manage terminal system notifications",
        args: "on|off|status",
    },
];

pub(crate) const AUDIT_SUBCOMMANDS: &[SubcommandSpec] = &[SubcommandSpec {
    name: "list",
    aliases: &["ls"],
    description: "List audit log entries",
    args: "",
}];

pub(crate) const REACTION_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "add",
        aliases: &[],
        description: "React to current thread/comment/DM",
        args: "emoji [index]",
    },
    SubcommandSpec {
        name: "remove",
        aliases: &["delete"],
        description: "Remove a reaction",
        args: "emoji [index]",
    },
];
