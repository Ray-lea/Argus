use crate::{Refineable, TextStyle, TextStyleRefinement};
use std::cell::RefCell;

/// Lazily resolve each inherited prefix once, instead of walking every ancestor
/// whenever a text element asks for its font, metrics, alignment or paint color.
#[derive(Clone, Default)]
pub(crate) struct TextStyleStack {
    refinements: Vec<TextStyleRefinement>,
    resolved: RefCell<Vec<TextStyle>>,
}

impl TextStyleStack {
    pub fn current(&self) -> TextStyle {
        let mut resolved = self.resolved.borrow_mut();
        for refinement in &self.refinements[resolved.len()..] {
            let mut style = resolved.last().cloned().unwrap_or_default();
            style.refine(refinement);
            resolved.push(style);
        }
        resolved.last().cloned().unwrap_or_default()
    }

    pub fn push(&mut self, style: TextStyleRefinement) {
        self.refinements.push(style);
    }

    pub fn pop(&mut self) {
        self.refinements.pop();
        self.resolved.get_mut().truncate(self.refinements.len());
    }

    pub fn clear(&mut self) {
        self.refinements.clear();
        self.resolved.get_mut().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FontWeight, px};

    #[test]
    fn cached_prefix_matches_composition_and_survives_sibling_changes() {
        let parent = TextStyleRefinement {
            font_size: Some(px(18.).into()),
            ..Default::default()
        };
        let child = TextStyleRefinement {
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        };
        let mut stack = TextStyleStack::default();
        let mut expected = TextStyle::default();
        stack.push(parent.clone());
        expected.refine(&parent);
        assert_eq!(stack.current(), expected);
        stack.push(child.clone());
        expected.refine(&child);
        assert_eq!(stack.current(), expected);
        assert_eq!(stack.current(), expected);
        stack.pop();
        let sibling = TextStyleRefinement {
            font_size: Some(px(24.).into()),
            ..Default::default()
        };
        stack.push(sibling.clone());
        let mut expected = TextStyle::default();
        expected.refine(&parent);
        expected.refine(&sibling);
        assert_eq!(stack.current(), expected);
        stack.clear();
        assert_eq!(stack.current(), TextStyle::default());
    }

    #[test]
    fn deferred_stack_clones_keep_independent_scopes() {
        let mut stack = TextStyleStack::default();
        stack.push(TextStyleRefinement {
            font_size: Some(px(18.).into()),
            ..Default::default()
        });
        let expected = stack.current();
        let mut deferred = stack.clone();
        stack.clear();
        assert_eq!(deferred.current(), expected);
        deferred.clone_from(&stack);
        assert_eq!(deferred.current(), TextStyle::default());
    }
}
