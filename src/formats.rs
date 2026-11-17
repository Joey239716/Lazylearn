//! Video formats. Each format has its own cast, script instructions and renderer.
//! All formats share the teaching style in `script::DEFAULT_STYLE`.

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// Two stick figures (teacher + student) in front of a chalkboard
    #[default]
    Stickman,
    /// Animated science explainer: cosmic backdrop, glowing shapes, witty narrator
    Explainer,
    /// Marker-on-whiteboard sketch video with a narrator
    Whiteboard,
    /// Two friends texting on a phone screen
    Chat,
}

pub const ALL: &[Format] = &[Format::Stickman, Format::Explainer, Format::Whiteboard, Format::Chat];

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Stickman => "stickman",
            Format::Explainer => "explainer",
            Format::Whiteboard => "whiteboard",
            Format::Chat => "chat",
        }
    }

    pub fn tagline(self) -> &'static str {
        match self {
            Format::Stickman => "teacher + student stick figures banter in front of a chalkboard",
            Format::Explainer => "cosmic flat-design science explainer with a witty narrator and glowing concept shapes",
            Format::Whiteboard => "a narrator sketches it out: marker text and doodles draw themselves",
            Format::Chat => "two friends text each other on a phone, one explaining to the other",
        }
    }

    /// Narrator-only formats don't have dialogue between two characters.
    pub fn narrated(self) -> bool {
        matches!(self, Format::Explainer | Format::Whiteboard)
    }

    pub fn uses_poses(self) -> bool {
        self == Format::Stickman
    }

    pub fn uses_visuals(self) -> bool {
        matches!(self, Format::Explainer | Format::Whiteboard)
    }

    /// Format-specific instructions: the cast, the voice of the script, and how
    /// to use the on-screen elements.
    pub fn prompt(self) -> &'static str {
        match self {
            Format::Stickman => "\
FORMAT: animated stick figure skit.
CAST
- teacher (name it \"Prof. Stix\" unless the material suggests a funnier fit): hyped, clear, loves a good analogy.
- student (\"Pip\" unless you pick a better fun name): curious and funny. Asks the questions a real beginner would, falls for common misconceptions so the teacher can fix them, and sometimes sums things up in their own words.
ON SCREEN
- A chalkboard shows each scene's heading and its board bullets, revealed as the scene plays.
- For every line give the speaker's pose and the listener's pose. Teacher mostly uses explain/point (point = gesturing at the chalkboard); student often uses think/confused/excited/shrug. Use wave in the very first line.
- Use speaker values \"teacher\" and \"student\" only.",

            Format::Explainer => "\
FORMAT: high-production animated science explainer (flat, colorful design, cosmic backdrop).
CAST
- A single narrator (speaker \"narrator\" for every line). The voice is witty, warm and quietly awe-struck: zooms out to the big picture (\"you are made of this\"), zooms in to the tiny details, uses clever metaphors, and drops an occasional dry joke or a slightly existential aside. Speaks directly to the viewer as \"you\".
- teacher_name: \"Narrator\". student_name: \"Blip\" (a small round mascot who reacts on screen; it never speaks).
ON SCREEN
- Each scene shows 1-5 \"visuals\": labelled glowing shapes that pop in one by one as the narration mentions them. Labels are 1-3 words. Pick a shape that fits (circle for things and cells, hexagon for molecules and structures, star for energy and key results, triangle for forces and warnings, square for containers and systems).
- Pick the layout that best shows how the visuals relate: flow (a process, A then B then C), compare (A versus B), cycle (a loop that returns to the start), or hub (the first visual is the central idea and the others are its parts).
- The board bullets are shown one at a time as a big key-point caption, so keep them short and quotable.
- Mention the visuals in the narration in the same order they are listed.",

            Format::Whiteboard => "\
FORMAT: whiteboard sketch video. A marker writes notes and draws doodles while a narrator talks.
CAST
- A single narrator (speaker \"narrator\" for every line): a friendly tutor thinking out loud while drawing (\"so let's draw this out\", \"circle that, it's on the exam\"). Upbeat and quick.
- teacher_name: \"Narrator\". student_name: \"You\".
ON SCREEN
- The board bullets are hand-written on the whiteboard in order as the narration reaches them, so write them as crisp notes a top student would copy down, including key definitions and formulas spelled out in words.
- Each scene also has 1-4 \"visuals\": hand-drawn labelled doodles connected by arrows. Labels are 1-3 words. Pick a layout: flow, compare, cycle or hub (the first visual is the center).
- Mention the notes and doodles in the narration in the same order they are listed.",

            Format::Chat => "\
FORMAT: text-message conversation on a phone screen (like viral \"text story\" videos), read aloud by two voices.
CAST
- teacher: the friend who gets it (pick a short, fun first name). Explains with memes, analogies and patience, in text-speak but with correct content.
- student: the friend cramming for the exam who knows nothing yet (pick a short, fun first name). Sends panicked or funny texts, asks the obvious questions, gets things wrong, and has \"OHHH\" moments.
STYLE
- Lines are text messages: very short (most under 15 words, never more than 25), several messages in a row from the same person is fine. Use 5-10 messages per scene. Casual tone, but spell everything out so text-to-speech reads it well: no abbreviations like \"u\" or \"rn\", no emoji, no symbols.
- The board bullets appear as a pinned sticky note of key points next to the phone.
- Use speaker values \"teacher\" and \"student\" only.",
        }
    }
}
