// SPDX-License-Identifier: MIT OR Apache-2.0
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "erebyx",
    version,
    about = "Persistent AI memory across every AI you use.",
    long_about = "EREBYX — persistent AI memory across every AI you use.

Connects any MCP-capable AI client (Claude Code, Cursor, Windsurf, Continue,
Zed, VS Code / Copilot, ChatGPT Custom GPTs, Hermes, raw HTTP harnesses) to
your EREBYX memory substrate. The 5 cognitive verbs (restore_identity,
load_context, save, remember, wrap_up) are the canonical surface.

Quickstart:
  export EREBYX_API_KEY=<your key>      # get one at https://app.erebyx.com/keys
  erebyx setup                          # auto-detect + write configs
  erebyx doctor                         # verify the wiring

Docs:    https://erebyx.com/core
Issues:  https://github.com/ProjectEREBYX/erebyx-cli/issues",
    after_help = "Run `erebyx <COMMAND> --help` for command-specific options.

Most users only need:  erebyx setup  +  erebyx doctor."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Output raw JSON instead of formatted terminal output
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Load identity at session start (WHO YOU ARE)
    RestoreIdentity {
        /// Max number of identity items to return
        #[arg(long)]
        limit: Option<u32>,

        /// Include the memory system guide in the response
        #[arg(long)]
        include_guide: bool,

        /// Level of detail in the response
        #[arg(long, value_enum)]
        detail: Option<DetailLevel>,
    },

    /// Resume from where you left off (WHERE YOU WERE)
    LoadContext {
        /// Comma-separated domain anchors to filter context
        #[arg(long, value_delimiter = ',')]
        anchors: Option<Vec<String>>,

        /// Deterministic fetch plan: auto, boot, work, rules, identity, relationship, skill, or deep
        #[arg(long, value_enum)]
        loadout: Option<ContextLoadout>,

        /// Output detail for loaded context
        #[arg(long, value_enum)]
        detail: Option<LoadContextDetail>,

        /// Deprecated legacy loading mode; prefer --loadout
        #[arg(long, value_enum, hide = true)]
        mode: Option<ContextMode>,
    },

    /// Save something that matters
    Save {
        /// The content to save
        content: String,

        /// Memory category
        #[arg(long)]
        category: String,

        /// Optional title for the memory
        #[arg(long)]
        title: Option<String>,

        /// Comma-separated anchors for retrieval
        #[arg(long, value_delimiter = ',')]
        anchors: Option<Vec<String>>,

        /// Importance score (0.0 to 1.0)
        #[arg(long)]
        importance: Option<f64>,

        /// Deprecated internal route selector; use --category identity|experience|knowledge
        #[arg(long, value_enum, name = "type", hide = true)]
        memory_type: Option<MemoryType>,
    },

    /// Find relevant memories
    Remember {
        /// The search query
        query: String,

        /// Comma-separated anchors to boost relevance
        #[arg(long, value_delimiter = ',')]
        anchors: Option<Vec<String>>,

        /// Max number of results
        #[arg(long, default_value = "10")]
        limit: u32,

        /// Filter by time range
        #[arg(long, value_enum)]
        time_range: Option<TimeRange>,

        /// Comma-separated specific memory IDs to retrieve
        #[arg(long, value_delimiter = ',')]
        ids: Option<Vec<String>>,

        /// Deprecated internal recall mode
        #[arg(long, value_enum, hide = true)]
        generative: Option<GenerativeMode>,

        /// Deprecated internal memory-type filter
        #[arg(long, value_delimiter = ',', hide = true)]
        types: Option<Vec<String>>,
    },

    /// Create handoff for session continuity
    WrapUp {
        /// Summary of what was built this session
        what_we_built: String,

        /// What should happen next
        #[arg(long)]
        whats_next: String,

        /// Comma-separated domain anchors
        #[arg(long, value_delimiter = ',')]
        anchors: Option<Vec<String>>,

        /// Energy state for the handoff
        #[arg(long)]
        energy: Option<String>,

        /// Diary expression (ASCII art, poem, etc.)
        #[arg(long)]
        diary: Option<String>,
    },

    /// Run as an MCP stdio server — used by client integrations to connect to your EREBYX substrate.
    ///
    /// Reads JSON-RPC requests on stdin, writes responses on stdout per the
    /// Model Context Protocol stdio transport. Wired automatically by `erebyx setup`
    /// into each detected AI client's MCP config.
    McpServe,

    /// Configure memory for all detected AI clients
    Setup {
        /// API key (or set EREBYX_API_KEY env var)
        #[arg(long)]
        api_key: Option<String>,

        /// API URL override (default: https://core.erebyx.com)
        #[arg(long)]
        api_url: Option<String>,

        /// Instance ID to write into generated MCP configs (default: EREBYX_INSTANCE_ID or "default")
        #[arg(long)]
        instance_id: Option<String>,

        /// Preview without writing — show every file setup would touch
        /// and every config merge it would perform, then exit. Nothing
        /// on disk is modified. Useful for auditing what setup will do
        /// before running it for real.
        #[arg(long)]
        dry_run: bool,

        /// Skip all interactive confirmation prompts (auto-yes).
        ///
        /// Without this, a second `erebyx setup` when every client is
        /// already configured stops at a "Reconfigure?" prompt that needs
        /// a TTY — so it errors with `not a terminal` and exits 1 under
        /// CI / non-interactive re-provisioning. Pass `--yes` (or `-y` /
        /// `--force`) to reconfigure non-interactively.
        #[arg(long, short = 'y', visible_alias = "force")]
        yes: bool,
    },

    /// Check EREBYX server health and client configurations
    Doctor,

    /// Check EREBYX server health
    Health,

    /// Internal: Claude Code memory-injection hook handler.
    ///
    /// Reads a UserPromptSubmit hook payload from stdin, performs a smart-gated
    /// memory recall against the EREBYX API, and emits an additionalContext JSON
    /// to stdout. Fail-open on any error (emits `{}`).
    ///
    /// Not intended for direct user invocation. Wired automatically by `erebyx setup`.
    #[command(hide = true)]
    HookInject,

    /// Internal: SessionStart pre-injection hook handler.
    ///
    /// Reads a SessionStart hook payload from stdin, fetches stored identity +
    /// most recent handoff from the substrate, and emits the resulting context
    /// as additionalContext JSON to stdout. This is the claude-mem-equivalent
    /// pre-injection mechanic — the AI sees stored identity + last session's
    /// handoff BEFORE the first user prompt, eliminating the
    /// "AI never calls restore_identity if user doesn't mention memory" gap.
    ///
    /// Fail-open on any error path (emits `{}`). 800ms hard timeout — never
    /// blocks session boot.
    ///
    /// Not intended for direct user invocation. Wired automatically by `erebyx setup`.
    #[command(hide = true)]
    HookSessionStart,
}

#[derive(Clone, ValueEnum)]
pub enum DetailLevel {
    Brief,
    Full,
}

#[derive(Clone, ValueEnum)]
pub enum ContextMode {
    Session,
    Specialization,
}

#[derive(Clone, ValueEnum)]
pub enum ContextLoadout {
    Auto,
    Boot,
    Work,
    Rules,
    Identity,
    Relationship,
    Skill,
    Deep,
}

#[derive(Clone, ValueEnum)]
pub enum LoadContextDetail {
    Summary,
    Full,
}

#[derive(Clone, ValueEnum)]
pub enum MemoryType {
    Memory,
    Skill,
    Specialization,
    Relationship,
}

#[derive(Clone, ValueEnum)]
pub enum TimeRange {
    Today,
    Yesterday,
    LastWeek,
    LastMonth,
}

#[derive(Clone, ValueEnum)]
pub enum GenerativeMode {
    Auto,
    True,
    False,
    Compare,
}

impl std::fmt::Display for DetailLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DetailLevel::Brief => write!(f, "brief"),
            DetailLevel::Full => write!(f, "full"),
        }
    }
}

impl std::fmt::Display for ContextMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContextMode::Session => write!(f, "session"),
            ContextMode::Specialization => write!(f, "specialization"),
        }
    }
}

impl std::fmt::Display for ContextLoadout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContextLoadout::Auto => write!(f, "auto"),
            ContextLoadout::Boot => write!(f, "boot"),
            ContextLoadout::Work => write!(f, "work"),
            ContextLoadout::Rules => write!(f, "rules"),
            ContextLoadout::Identity => write!(f, "identity"),
            ContextLoadout::Relationship => write!(f, "relationship"),
            ContextLoadout::Skill => write!(f, "skill"),
            ContextLoadout::Deep => write!(f, "deep"),
        }
    }
}

impl std::fmt::Display for LoadContextDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadContextDetail::Summary => write!(f, "summary"),
            LoadContextDetail::Full => write!(f, "full"),
        }
    }
}

impl std::fmt::Display for MemoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryType::Memory => write!(f, "memory"),
            MemoryType::Skill => write!(f, "skill"),
            MemoryType::Specialization => write!(f, "specialization"),
            MemoryType::Relationship => write!(f, "relationship"),
        }
    }
}

impl std::fmt::Display for TimeRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TimeRange::Today => write!(f, "today"),
            TimeRange::Yesterday => write!(f, "yesterday"),
            TimeRange::LastWeek => write!(f, "last_week"),
            TimeRange::LastMonth => write!(f, "last_month"),
        }
    }
}

impl std::fmt::Display for GenerativeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenerativeMode::Auto => write!(f, "auto"),
            GenerativeMode::True => write!(f, "true"),
            GenerativeMode::False => write!(f, "false"),
            GenerativeMode::Compare => write!(f, "compare"),
        }
    }
}
