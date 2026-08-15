//! Where the rows of a settings card sit, and how tall the card is.
//!
//! The System page (design §3.3) and the About page (design §3.4) are each ONE
//! card whose height is its contents' rather than the window's, so "how tall is
//! that card" is arithmetic over a row rhythm and nothing else -- no `HWND`, no
//! DPI call, no Win32 anywhere. It lived in
//! `beckon-windows/src/settings_window/layout.rs`, which is `cfg`-gated to
//! Windows and had **zero tests**; the whole vertical geometry of the window
//! was, measured across the tree, untestable on two of the three CI jobs and
//! unrunnable on the machine it is written on.
//!
//! That is the same argument design §12 q3 makes for `Page` living in core, and
//! it is why the defect these functions were changed for went unnoticed: at the
//! shipped window height the System card ended 224 px above the command bar and
//! the About card 210 px above it -- a third of the window, on two doors out of
//! four -- and nothing anywhere could fail.
//!
//! **This module owns the RHYTHM, not the sizes.** Every length arrives in
//! `RowMetrics`, already scaled to the live DPI by the caller, because the
//! tokens are the window's (`layout.rs`'s `mod tok`) and a second copy here is
//! exactly the drift the one-plan-three-readers rule exists to stop.

/// One page's vertical lengths, at ONE DPI, already scaled.
///
/// The caller scales; this module only stacks. A `dpi` field would invite a
/// second `v * dpi / 96` beside the window's own and give the two a chance to
/// round differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowMetrics {
    /// One control line -- `tok::CTL`.
    pub ctl: i32,
    /// Between two rows of the same group.
    ///
    /// **This is `tok::ROW_GAP` (20 at 96 DPI), not `tok::GAP` (6), since
    /// 2026-08-15.** The rows used to sit one `GAP` apart, giving a 32 px
    /// pitch against the mock-up's 46, which is where 110 px of the System
    /// card's shortfall came from. It is a setting-row rhythm, deliberately
    /// not the gap between a label and the control beside it.
    pub row_gap: i32,
    /// Above and below a divider's hairline -- `tok::DIV_GAP`.
    ///
    /// Smaller than `row_gap` on purpose, and the drawing says so: a group
    /// boundary reads as a boundary because of the LINE, so spending a full
    /// row gap on each side as well parts the groups twice. At 96 DPI the
    /// boundary is `10 + 1 + 10 = 21` against a 20 px row gap.
    pub div_gap: i32,
    /// The About mark's side -- `paint::MARK_D`.
    pub mark: i32,
    /// The About name line. Not `ctl`: it is text, not a control.
    pub name: i32,
}

impl RowMetrics {
    /// The pitch from one row's top to the next's.
    pub fn pitch(self) -> i32 {
        self.ctl + self.row_gap
    }

    /// A divider's whole footprint: a hairline with a gap either side.
    fn divider(self) -> i32 {
        self.div_gap * 2 + 1
    }
}

/// Which of the System page's two CONDITIONAL rows are on this machine.
///
/// `Start with Windows` is absent under `beckon.exe serve`, which cannot write
/// a Run value, and the log row is absent when `serve` ran without `--log`.
/// Both are "omitted, not greyed" (design §3.3), which is a LAYOUT property:
/// an absent row contributes no height and the rows below it close up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SystemRows {
    pub autostart: bool,
    pub log: bool,
}

/// Where each of the System card's rows starts, as an offset from the card's
/// own content origin, plus the two dividers and the total interior height.
///
/// **One plan, three readers**: `compute_card_rects` takes `content_h` to size
/// the card, `layout` takes the row offsets to place fourteen controls, and
/// `WM_PAINT` takes the two divider offsets to draw two hairlines. Three
/// spellings of "how tall is the System card" would drift, and the drift would
/// read as a rendering fault -- a divider through a row, or a card with a gap
/// at the bottom.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SystemPlan {
    pub pause: i32,
    pub autostart: i32,
    pub reload: i32,
    pub div1: i32,
    pub dark: i32,
    pub opacity: i32,
    pub div2: i32,
    pub config: i32,
    pub log: i32,
    pub content_h: i32,
}

/// Walk the nine slots in drawing order, skipping the two conditional ones.
///
/// The order IS the mock-up's, top to bottom: the three service rows, a
/// divider, the two look rows, a divider, the two file rows. Design §3.3 calls
/// it five rows; nine slots is the same page counted by control line rather
/// than by group, and the two dividers are what turn nine lines into three
/// groups without a heading on any of them -- design §7 rule 5, read backwards:
/// a group whose rows share a store does not need a word saying so.
pub fn system_plan(m: RowMetrics, rows: SystemRows) -> SystemPlan {
    let mut p = SystemPlan::default();
    let mut y = 0;
    let row = |y: &mut i32, on: bool| -> i32 {
        if !on {
            // The offset an absent row reports is where the NEXT row starts,
            // so a caller that places it anyway (which `layout` does not)
            // stacks it under its successor rather than at the card's origin.
            return *y;
        }
        let at = *y;
        *y += m.pitch();
        at
    };
    p.pause = row(&mut y, true);
    p.autostart = row(&mut y, rows.autostart);
    p.reload = row(&mut y, true);
    // The gap the last row of a group already left is taken back before the
    // divider is placed -- otherwise every group boundary would be spaced
    // twice, once by the row rhythm and once by the divider's own inset.
    y -= m.row_gap;
    p.div1 = y + m.div_gap;
    y += m.divider();
    p.dark = row(&mut y, true);
    p.opacity = row(&mut y, true);
    y -= m.row_gap;
    p.div2 = y + m.div_gap;
    y += m.divider();
    p.config = row(&mut y, true);
    p.log = row(&mut y, rows.log);
    // The trailing gap the last row added is not interior height: the card's
    // own `CARD_PAD` is what separates it from the border.
    p.content_h = (y - m.row_gap).max(0);
    p
}

/// Every vertical figure on the About page, at one DPI.
///
/// `system_plan`'s shape exactly, and for the same three readers.
///
/// **Nothing here is conditional**, unlike System's two omittable rows: there
/// is no fact about a machine that removes a row from this page, so the plan
/// takes no `rows` argument and only the disclosure's height varies -- with the
/// FONT and the card's width, not with any state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AboutPlan {
    pub mark: i32,
    pub name: i32,
    pub div1: i32,
    pub build: i32,
    pub location: i32,
    pub licence: i32,
    pub div2: i32,
    pub disclosure: i32,
    pub links: i32,
    pub content_h: i32,
}

/// `disclosure_h` is measured by the window (`DT_CALCRECT`, the painter's own
/// flags) and passed in, so this function stays pure arithmetic and the one
/// measurement has one call site per pass.
pub fn about_plan(m: RowMetrics, disclosure_h: i32) -> AboutPlan {
    let mut p = AboutPlan::default();
    let mut y = 0;

    // The identity block: the mark, then the name under it. `mark` is
    // `paint::mark`'s own constant -- the painter draws a tile that size and
    // this reserves exactly it.
    //
    // **The two are `row_gap` apart, not `ctl`-pitched**: neither is a row, so
    // neither takes a row's pitch, but the block still has to breathe with the
    // same rhythm as the rows under it.
    p.mark = y;
    y += m.mark + m.row_gap;
    p.name = y;
    y += m.name;

    p.div1 = y + m.div_gap;
    y += m.divider();

    let row = |y: &mut i32| -> i32 {
        let at = *y;
        *y += m.pitch();
        at
    };
    p.build = row(&mut y);
    p.location = row(&mut y);
    p.licence = row(&mut y);
    // The gap the last row left is taken back before the divider, exactly as
    // `system_plan` does and for the same reason.
    y -= m.row_gap;
    p.div2 = y + m.div_gap;
    y += m.divider();

    p.disclosure = y;
    y += disclosure_h + m.row_gap;
    p.links = y;
    y += m.ctl;
    p.content_h = y.max(0);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped tokens at 96 DPI. Spelled here rather than imported
    /// because `mod tok` is inside a `cfg(windows)` crate; `layout.rs`'s
    /// `the_row_rhythm_matches_core` is what holds the two together.
    const M96: RowMetrics = RowMetrics {
        ctl: 26,
        row_gap: 20,
        div_gap: 10,
        mark: 36,
        name: 24,
    };

    const BOTH: SystemRows = SystemRows {
        autostart: true,
        log: true,
    };

    /// The rhythm itself, which is what the 2026-08-15 change moved: 46 px
    /// from one row's top to the next's, where it was 32.
    ///
    /// Asserted as a DIFFERENCE between real offsets rather than as
    /// `ctl + row_gap`, which would restate `pitch`'s body and pass for any
    /// rhythm at all.
    #[test]
    fn rows_sit_one_pitch_apart() {
        let p = system_plan(M96, BOTH);
        assert_eq!(M96.pitch(), 46);
        assert_eq!(p.autostart - p.pause, 46);
        assert_eq!(p.reload - p.autostart, 46);
        assert_eq!(p.opacity - p.dark, 46);
        assert_eq!(p.log - p.config, 46);

        let a = about_plan(M96, 32);
        assert_eq!(a.location - a.build, 46);
        assert_eq!(a.licence - a.location, 46);
    }

    /// A group boundary parts its groups by MORE than a row gap, and by only
    /// a little more. Both halves matter: equal and the divider is invisible
    /// in the rhythm, much larger and the card gains a hole at every group.
    #[test]
    fn a_divider_parts_its_groups_by_slightly_more_than_a_row_gap() {
        let p = system_plan(M96, BOTH);
        let reload_bottom = p.reload + M96.ctl;
        assert_eq!(p.dark - reload_bottom, 21);
        assert!(p.dark - reload_bottom > M96.row_gap);
        assert!(p.dark - reload_bottom < M96.row_gap * 2);
        // The hairline sits inside that boundary, not on a row.
        assert!(p.div1 > reload_bottom && p.div1 < p.dark);
        assert_eq!(p.div1 - reload_bottom, M96.div_gap);
    }

    /// The System card's interior, which is what `compute_card_rects` adds
    /// `CARD_PAD` twice to. **304 at 96 DPI with both conditional rows up**,
    /// where the pre-2026-08-15 rhythm gave 232.
    ///
    /// A number, not a formula: a formula here would be the function's own
    /// body written twice and would pass whatever the body said.
    #[test]
    fn the_system_card_interior_is_304_with_every_row() {
        assert_eq!(system_plan(M96, BOTH).content_h, 304);
    }

    /// An omitted row costs nothing, which is what makes "omitted, not
    /// greyed" a layout property rather than a `ShowWindow` leaving a hole.
    #[test]
    fn an_absent_row_gives_its_height_back() {
        let both = system_plan(M96, BOTH);
        let neither = system_plan(M96, SystemRows::default());
        assert_eq!(both.content_h - neither.content_h, 2 * M96.pitch());
        // ...and the rows below it move up by exactly that much.
        assert_eq!(both.reload - neither.reload, M96.pitch());
        assert_eq!(both.config - neither.config, M96.pitch());
        // An absent row reports where its successor starts.
        assert_eq!(neither.autostart, neither.reload);
    }

    /// The About card's interior at the two-line disclosure the shipped
    /// string wraps to at 96 DPI, and the fact the window's floor is derived
    /// from: the card grows one line at a time and nothing else moves.
    #[test]
    fn the_about_card_interior_grows_only_with_the_disclosure() {
        let two = about_plan(M96, 32);
        assert_eq!(two.content_h, 318);
        let three = about_plan(M96, 48);
        assert_eq!(three.content_h, 334);
        assert_eq!(three.content_h - two.content_h, 16);
        // Everything above the disclosure is fixed, so only the links row
        // moves with it.
        assert_eq!(two.build, three.build);
        assert_eq!(two.disclosure, three.disclosure);
        assert_eq!(three.links - two.links, 16);
    }

    /// The two pages land within a row of each other, which is why one window
    /// height can serve both without one of them carrying a hole. Not a
    /// coincidence worth relying on -- an assertion, so that a row added to
    /// either page has to be weighed against the other.
    #[test]
    fn the_two_fixed_pages_are_the_same_height_within_one_row() {
        let sys = system_plan(M96, BOTH).content_h;
        let about = about_plan(M96, 32).content_h;
        assert!(
            (sys - about).abs() <= M96.pitch(),
            "System is {sys} and About is {about}; one door now carries a \
             hole the other does not, and `MIN_HEIGHT` is derived from the \
             taller of the two"
        );
    }

    /// Offsets ascend in drawing order. A plan that stacked two rows at one
    /// offset would draw them on top of each other, and every assertion above
    /// could still hold.
    #[test]
    fn every_offset_ascends_in_drawing_order() {
        let p = system_plan(M96, BOTH);
        let sys = [
            p.pause,
            p.autostart,
            p.reload,
            p.div1,
            p.dark,
            p.opacity,
            p.div2,
            p.config,
            p.log,
        ];
        for w in sys.windows(2) {
            assert!(w[0] < w[1], "System offsets go backwards: {sys:?}");
        }
        assert!(p.log + M96.ctl <= p.content_h);

        let a = about_plan(M96, 32);
        let about = [
            a.mark,
            a.name,
            a.div1,
            a.build,
            a.location,
            a.licence,
            a.div2,
            a.disclosure,
            a.links,
        ];
        for w in about.windows(2) {
            assert!(w[0] < w[1], "About offsets go backwards: {about:?}");
        }
        assert!(a.links + M96.ctl <= a.content_h);
    }
}
