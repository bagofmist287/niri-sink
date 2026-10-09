use smithay::utils::{Logical, Point, Rectangle, Size};

use super::scrolling::{ColumnWidth, WindowHeight};
use super::tile::Tile;
use super::LayoutElement;

/// The normal layout to restore a tile to, without recreating its former column or tab group.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RestorePlacement {
    Scrolling {
        width: ColumnWidth,
        is_full_width: bool,
        height: WindowHeight,
    },
    Floating {
        width: ColumnWidth,
    },
}

impl RestorePlacement {
    pub fn is_floating(self) -> bool {
        matches!(self, Self::Floating { .. })
    }

    pub fn width(self) -> ColumnWidth {
        match self {
            Self::Scrolling { width, .. } | Self::Floating { width } => width,
        }
    }

    pub fn is_full_width(self) -> bool {
        matches!(
            self,
            Self::Scrolling {
                is_full_width: true,
                ..
            }
        )
    }
}

/// Native, unscaled geometry captured before removing a tile from its normal layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SourceGeometry {
    pub tile: Rectangle<f64, Logical>,
    pub window_size: Size<i32, Logical>,
}

/// The sole owner of a sunk tile. Projection geometry never replaces its client geometry.
#[derive(Debug)]
pub struct SinkEntry<W: LayoutElement> {
    tile: Tile<W>,
    pub restore: RestorePlacement,
    pub source: SourceGeometry,
}

impl<W: LayoutElement> SinkEntry<W> {
    pub fn new(
        tile: Tile<W>,
        restore: RestorePlacement,
        source_position: Point<f64, Logical>,
    ) -> Self {
        let source = SourceGeometry {
            tile: Rectangle::new(source_position, tile.tile_size()),
            window_size: tile.window().size(),
        };
        Self {
            tile,
            restore,
            source,
        }
    }

    pub fn tile(&self) -> &Tile<W> {
        &self.tile
    }

    pub fn tile_mut(&mut self) -> &mut Tile<W> {
        &mut self.tile
    }

    /// Refreshes source dimensions after an actual native window or decoration update.
    pub fn update_source_size(&mut self) {
        self.source.tile.size = self.tile.tile_size();
        self.source.window_size = self.tile.window().size();
    }

    pub fn into_parts(self) -> (Tile<W>, RestorePlacement, SourceGeometry) {
        (self.tile, self.restore, self.source)
    }
}

/// A workspace-local LIFO owner. The end of the vector is the top of the stack.
#[derive(Debug)]
pub struct SinkSpace<W: LayoutElement> {
    entries: Vec<SinkEntry<W>>,
}

impl<W: LayoutElement> Default for SinkSpace<W> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<W: LayoutElement> SinkSpace<W> {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn depth(&self, id: &W::Id) -> Option<usize> {
        self.entries
            .iter()
            .rev()
            .position(|entry| entry.tile.window().id() == id)
    }

    /// Rejects a duplicate ID while returning the incoming tile to its caller.
    pub fn push(&mut self, entry: SinkEntry<W>) -> Result<(), Box<SinkEntry<W>>> {
        if self.depth(entry.tile.window().id()).is_some() {
            return Err(Box::new(entry));
        }
        self.entries.push(entry);
        Ok(())
    }

    pub fn pop(&mut self) -> Option<SinkEntry<W>> {
        self.entries.pop()
    }

    pub fn remove(&mut self, id: &W::Id) -> Option<SinkEntry<W>> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.tile.window().id() == id)?;
        Some(self.entries.remove(index))
    }

    pub fn get(&self, id: &W::Id) -> Option<&SinkEntry<W>> {
        self.entries
            .iter()
            .find(|entry| entry.tile.window().id() == id)
    }

    pub fn get_mut(&mut self, id: &W::Id) -> Option<&mut SinkEntry<W>> {
        self.entries
            .iter_mut()
            .find(|entry| entry.tile.window().id() == id)
    }

    pub fn tiles(&self) -> impl Iterator<Item = &Tile<W>> {
        self.entries.iter().map(SinkEntry::tile)
    }

    pub fn tiles_mut(&mut self) -> impl Iterator<Item = &mut Tile<W>> {
        self.entries.iter_mut().map(SinkEntry::tile_mut)
    }

    /// Enumerates entries in presentation order, with the current top at depth zero.
    pub fn entries(&self) -> impl Iterator<Item = (usize, &SinkEntry<W>)> {
        self.entries.iter().rev().enumerate()
    }
}
