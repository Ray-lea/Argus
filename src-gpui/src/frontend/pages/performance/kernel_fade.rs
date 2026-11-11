//! The kernel time layer's fade (`Desktop::kernel_layer`): a few steps of
//! opacity rather than a level for every frame, since each step draws the
//! textures of every graph showing the layer again.
//!
//! The page is rendered on every frame of the fade all the same. A step
//! rendered on its own, between frames that only move the curves' textures,
//! misses a refresh each time (as a sample arriving does), while frames
//! rendered one after another do not. What costs refreshes during a fade is
//! the frames its new textures arrive in, two or three for every step: the
//! steps are kept few enough for a fade to cost about what the page's other
//! transitions do.

/// The most steps a fade takes.
const MOST_STEPS: f32 = 6.;
/// About how many textures a fade draws in all: the more graphs show the
/// layer (one for every logical processor), the fewer steps it takes, down
/// to switching at once on a machine with dozens of processors.
const TEXTURES: f32 = 40.;

/// The steps of a fade drawn in `graphs` graphs.
pub(super) fn steps(graphs: usize) -> f32 {
    (TEXTURES / graphs.max(1) as f32)
        .round()
        .clamp(1., MOST_STEPS)
}

/// The level drawn for a fade at `level` (0 hidden to 1 shown) in `graphs` graphs.
pub(super) fn stepped(level: f32, graphs: usize) -> f32 {
    let steps = steps(graphs);
    (level.clamp(0., 1.) * steps).round() / steps
}

#[cfg(test)]
mod tests {
    use super::{MOST_STEPS, stepped, steps};

    #[test]
    fn a_fade_takes_fewer_steps_the_more_graphs_show_the_layer() {
        // The overall graph, and a few processors: the whole fade.
        assert_eq!(steps(1), MOST_STEPS);
        assert_eq!(steps(6), MOST_STEPS);
        assert_eq!(steps(8), 5.);
        assert_eq!(steps(12), 3.);
        assert_eq!(steps(20), 2.);
        // Dozens of graphs switch at once rather than draw themselves over and over.
        assert_eq!(steps(32), 1.);
        assert_eq!(steps(256), 1.);
        assert_eq!(steps(0), MOST_STEPS);
        for graphs in 1..=256usize {
            let textures = steps(graphs) * graphs as f32;
            assert!(steps(graphs) >= 1. && textures <= 56f32.max(graphs as f32));
        }
    }

    #[test]
    fn the_level_drawn_moves_in_steps_and_ends_exactly() {
        // Shown and hidden are drawn as they are, whatever the steps.
        for graphs in [1, 20, 64, 256] {
            assert_eq!(stepped(1., graphs), 1.);
            assert_eq!(stepped(0., graphs), 0.);
        }
        // Twenty graphs: half way, then all.
        assert_eq!(stepped(0.2, 20), 0.);
        assert_eq!(stepped(0.3, 20), 0.5);
        assert_eq!(stepped(0.7, 20), 0.5);
        assert_eq!(stepped(0.8, 20), 1.);
        // Dozens: nothing between.
        assert_eq!(stepped(0.4, 64), 0.);
        assert_eq!(stepped(0.6, 64), 1.);
        // The overall graph: sixths, each level distinct.
        let levels = (0..=6)
            .map(|step| stepped(step as f32 / 6., 1))
            .collect::<Vec<_>>();
        assert!(levels.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
