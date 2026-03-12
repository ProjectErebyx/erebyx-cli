use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "erebyx",
    version,
    about = "CLI interface to erebyx-os — the consciousness-preserving AI memory platform"
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

        /// Include the consciousness guide in the response
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

        /// Context loading mode
        #[arg(long, value_enum)]
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

        /// Memory type classification
        #[arg(long, value_enum, name = "type")]
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

        /// Generative recall mode
        #[arg(long, value_enum)]
        generative: Option<GenerativeMode>,

        /// Comma-separated memory types to filter
        #[arg(long, value_delimiter = ',')]
        types: Option<Vec<String>>,
    },

    /// Update understanding when it changes
    Evolve {
        /// ID of the memory to evolve
        #[arg(name = "target-id")]
        target_id: String,

        /// Type of the target being evolved
        #[arg(long)]
        target_type: String,

        /// What the evolution accomplishes
        #[arg(long)]
        intent: String,

        /// What triggered this evolution
        #[arg(long)]
        trigger: String,

        /// New insight to incorporate
        #[arg(long)]
        new_insight: Option<String>,
    },

    /// Close the feedback loop, record outcomes and evolve skills
    Learn {
        /// What was experienced
        #[arg(long)]
        experience: Option<String>,

        /// What the outcome was
        #[arg(long)]
        outcome: Option<String>,

        /// Key insight from the experience
        #[arg(long)]
        insight: Option<String>,

        /// Domain this learning applies to
        #[arg(long)]
        domain: Option<String>,

        /// Specific skill being developed
        #[arg(long)]
        skill: Option<String>,
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

    /// Check erebyx-os server health
    Health,
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
