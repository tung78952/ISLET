//! Who has the notch. Features overlap all the time (a card, then Caps Lock, while the notch steps
//! aside for typing), so what may appear is decided here, in one place, and not by each feature.

/// Where the big eyes may be drawn. Cards draw their own, smaller ones.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Seat {
    None,
    /// In the plain compact notch.
    Bar,
    /// Over the lyric bar.
    Lyrics,
    /// In the sliver left while the notch steps aside.
    Aside,
}

#[derive(Clone, Copy, Default)]
pub struct Scene {
    /// A toast is up, or an acted card is still fading out.
    pub toast: bool,
    pub yielded: bool,
    pub lyrics: bool,
}

pub fn seat(s: Scene) -> Seat {
    match s {
        Scene { toast: true, .. } => Seat::None,
        Scene { yielded: true, .. } => Seat::Aside,
        Scene { lyrics: true, .. } => Seat::Lyrics,
        _ => Seat::Bar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_toast_keeps_the_big_eyes_away_whatever_else_is_going_on() {
        for yielded in [false, true] {
            for lyrics in [false, true] {
                assert_eq!(seat(Scene { toast: true, yielded, lyrics }), Seat::None);
            }
        }
    }

    #[test]
    fn stepping_aside_comes_before_the_lyrics() {
        assert_eq!(seat(Scene { toast: false, yielded: true, lyrics: true }), Seat::Aside);
        assert_eq!(seat(Scene { toast: false, yielded: false, lyrics: true }), Seat::Lyrics);
        assert_eq!(seat(Scene::default()), Seat::Bar);
    }
}
