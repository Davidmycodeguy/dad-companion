//! Fakes shared by the runner tests, which include this file with `#[path = "runner_fakes.rs"]`:
//! a driver that records what would have been clicked, scripted game answers and safety checks —
//! ports of the fakes in DnDTools' `test_marketplace_runner.py` / `test_merchant_runner.py`.
//! Nothing here touches the real mouse or keyboard. (Cargo also builds this file as a test binary
//! of its own, with no tests.)
#![allow(dead_code)]

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use input::marketplace::{build_layout, MarketplaceLayout};
use lister::job::{CancelToken, ItemResult, RunReport};
use lister::marketplace_state::{ListingsSnapshot, MarketplaceState, RegisterOutcome, FIRST_PAGE};
use lister::merchant_state::{SellBack, SELL_SUCCESS};
use lister::plan::PlanEntry;
use lister::runner::merchant::MERCHANT_CARD_INDEX;
use lister::runner::{
    GameInput, InputError, MarketplaceGame, MarketplaceRunner, MerchantGame, MerchantSaleRunner, SafetyCheck,
    ScreenPoint,
};
use market::MarketRow;
use serde_json::Value;

/// The Marketplace tab order the Marketplace tests use.
pub const MAPPING: [i32; 8] = [4, 20, 5, 6, 7, 8, 9, 30];

/// The 1920x1080 layout, uncalibrated.
pub fn layout() -> &'static MarketplaceLayout {
    static LAYOUT: OnceLock<MarketplaceLayout> = OnceLock::new();
    LAYOUT.get_or_init(|| build_layout((1920, 1080), (0, 0), &Value::Null))
}

pub fn point(key: &str) -> ScreenPoint {
    layout().point(key)
}

/// Every flow starts by re-opening My Listings (via View Market) and waiting for the game to
/// confirm it.
pub fn verify_clicks() -> Vec<ScreenPoint> {
    vec![point("view_market_tab"), point("my_listings_tab")]
}

/// A Marketplace plan entry (`_entry` in the Python tests).
pub fn entry(uid: &str, stash: &str, slot: i64, price: i64) -> PlanEntry {
    PlanEntry::new(uid, format!("Item {uid}"), 5, stash, slot, 1, 1, price, 45, 10)
}

pub fn row(item_id: &str, price: i64, listing_id: &str) -> MarketRow {
    MarketRow { item_id: item_id.to_string(), price, base: vec![], rolls: vec![], listing_id: listing_id.to_string(), count: 1 }
}

/// A full result page of item `X_5001` (listing ids 0..9).
pub fn full_page() -> Vec<MarketRow> {
    (0..10).map(|i| row("X_5001", 100 + i, &i.to_string())).collect()
}

pub fn statuses(report: &RunReport) -> Vec<&str> {
    report.results.iter().map(|r| r.status.as_str()).collect()
}

pub fn uid_statuses(report: &RunReport) -> Vec<(&str, &str)> {
    report.results.iter().map(|r| (r.unique_id.as_str(), r.status.as_str())).collect()
}

/// One thing the fake driver was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Click(ScreenPoint),
    Move(ScreenPoint),
    Type(String),
    Drag(ScreenPoint, ScreenPoint),
    Escape,
    /// A `snapshot_position()` call, recorded by [`FakeSafety::recording`].
    Snapshot,
}

type Hook = Arc<dyn Fn(&Action, &FakeDriver) + Send + Sync>;
type FailHook = Arc<dyn Fn(&Action) -> Option<InputError> + Send + Sync>;

/// Records every action instead of sending it; hooks let a fake game (or a test) react to them.
#[derive(Default)]
pub struct FakeDriver {
    actions: Mutex<Vec<Action>>,
    pos: Mutex<ScreenPoint>,
    hooks: Mutex<Vec<Hook>>,
    fail: Mutex<Option<FailHook>>,
}

impl FakeDriver {
    pub fn new() -> Arc<Self> {
        Arc::new(FakeDriver::default())
    }

    pub fn actions(&self) -> Vec<Action> {
        self.actions.lock().unwrap().clone()
    }

    pub fn clicks(&self) -> Vec<ScreenPoint> {
        self.actions().into_iter().filter_map(|a| if let Action::Click(p) = a { Some(p) } else { None }).collect()
    }

    pub fn count(&self, action: &Action) -> usize {
        self.actions().iter().filter(|a| *a == action).count()
    }

    pub fn clicked(&self, point: ScreenPoint) -> bool {
        self.clicks().contains(&point)
    }

    pub fn record(&self, action: Action) {
        self.actions.lock().unwrap().push(action);
    }

    pub fn pos(&self) -> ScreenPoint {
        *self.pos.lock().unwrap()
    }

    pub fn set_pos(&self, pos: ScreenPoint) {
        *self.pos.lock().unwrap() = pos;
    }

    /// Runs `hook` after every action (recorded, cursor moved), in the order hooks were added.
    pub fn on_action(&self, hook: impl Fn(&Action, &FakeDriver) + Send + Sync + 'static) {
        self.hooks.lock().unwrap().push(Arc::new(hook));
    }

    /// Makes an action fail (unrecorded) whenever `fail` returns an error for it.
    pub fn fail_when(&self, fail: impl Fn(&Action) -> Option<InputError> + Send + Sync + 'static) {
        *self.fail.lock().unwrap() = Some(Arc::new(fail));
    }

    fn act(&self, action: Action, moved_to: Option<ScreenPoint>) -> Result<(), InputError> {
        let fail = self.fail.lock().unwrap().clone();
        if let Some(error) = fail.and_then(|fail| fail(&action)) {
            return Err(error);
        }
        self.record(action.clone());
        if let Some(pos) = moved_to {
            self.set_pos(pos);
        }
        let hooks = self.hooks.lock().unwrap().clone();
        for hook in hooks {
            hook(&action, self);
        }
        Ok(())
    }
}

impl GameInput for FakeDriver {
    fn click(&self, point: ScreenPoint) -> Result<(), InputError> {
        self.act(Action::Click(point), Some(point))
    }

    fn move_to(&self, point: ScreenPoint) -> Result<(), InputError> {
        self.act(Action::Move(point), Some(point))
    }

    fn drag(&self, from: ScreenPoint, to: ScreenPoint) -> Result<(), InputError> {
        self.act(Action::Drag(from, to), Some(to))
    }

    fn clear_and_type(&self, text: &str) -> Result<(), InputError> {
        self.act(Action::Type(text.to_string()), None)
    }

    fn press_escape(&self) -> Result<(), InputError> {
        self.act(Action::Escape, None)
    }

    fn position(&self) -> Result<ScreenPoint, InputError> {
        Ok(self.pos())
    }
}

/// A scripted safety check: passes `ok_calls` checkpoints (all, if `None`), then fails, optionally
/// taking a new reason; can record `snapshot_position()` calls into a driver's actions.
#[derive(Default)]
pub struct FakeSafety {
    reason: Mutex<Option<String>>,
    ok_calls: Option<usize>,
    reason_on_fail: Option<String>,
    calls: AtomicUsize,
    record_to: Option<Arc<FakeDriver>>,
}

impl FakeSafety {
    pub fn ok() -> Arc<Self> {
        Arc::new(FakeSafety::default())
    }

    /// Fails every checkpoint, with `reason` from the start (`FailSafety`).
    pub fn failing(reason: &str) -> Arc<Self> {
        Arc::new(FakeSafety { reason: Mutex::new(Some(reason.to_string())), ok_calls: Some(0), ..Default::default() })
    }

    /// Passes `ok_calls` checkpoints then fails, `reason` all along (`CountingSafety`).
    pub fn counting(ok_calls: usize, reason: &str) -> Arc<Self> {
        Arc::new(FakeSafety { reason: Mutex::new(Some(reason.to_string())), ok_calls: Some(ok_calls), ..Default::default() })
    }

    /// Passes `ok_calls` checkpoints, then fails as the game losing focus (the merchant `Safety`).
    pub fn failing_after(ok_calls: usize) -> Arc<Self> {
        Arc::new(FakeSafety { ok_calls: Some(ok_calls), reason_on_fail: Some("game_window_unfocused".into()), ..Default::default() })
    }

    /// Records each `snapshot_position()` into `driver` (`RecordingSafety`).
    pub fn recording(driver: &Arc<FakeDriver>, reason: Option<&str>, ok: bool) -> Arc<Self> {
        Arc::new(FakeSafety {
            reason: Mutex::new(reason.map(str::to_string)),
            ok_calls: if ok { None } else { Some(0) },
            record_to: Some(Arc::clone(driver)),
            ..Default::default()
        })
    }
}

impl SafetyCheck for FakeSafety {
    fn reason(&self) -> Option<String> {
        self.reason.lock().unwrap().clone()
    }

    fn checkpoint(&self) -> bool {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        match self.ok_calls {
            Some(ok_calls) if call > ok_calls => {
                if let Some(reason) = &self.reason_on_fail {
                    *self.reason.lock().unwrap() = Some(reason.clone());
                }
                false
            }
            _ => true,
        }
    }

    fn snapshot_position(&self) {
        if let Some(driver) = &self.record_to {
            driver.record(Action::Snapshot);
        }
    }
}

/// A fake game that reacts to clicks (page turns, tab switches).
pub trait ClickListener {
    fn on_click(&self, point: ScreenPoint);
}

impl ClickListener for MarketplaceState {
    fn on_click(&self, _point: ScreenPoint) {}
}

/// `range(used, 40)`: spots `0..used` are taken.
pub fn used(count: i64) -> Vec<i64> {
    (count..40).collect()
}

/// Answers the Marketplace runner from a script (`ScriptedState` + `PricingState`); build one with
/// struct update syntax over [`script`], then [`Script::build`].
pub struct Script {
    pub now: f64,
    pub snapshot: ListingsSnapshot,
    pub outcomes: VecDeque<RegisterOutcome>,
    pub confirm: bool,
    /// The page the game shows when My Listings opens.
    pub answers_page: i64,
    /// False: the game never answers.
    pub responsive: bool,
    /// The page the game shows after a listing.
    pub after_listing_page: Option<i64>,
    /// False: the page arrows do nothing.
    pub turns_pages: bool,
    pub shown_page: i64,
    /// Each search's first page (or a later page), in order; `None` never arrives.
    pub searches: VecDeque<Option<Vec<MarketRow>>>,
    /// `(currentPage, maxPage)` shown with each non-empty page.
    pub page_numbers: VecDeque<(i64, i64)>,
    pub shown_numbers: Option<(i64, i64)>,
    /// Runs after each result page is served (e.g. the player grabbing the mouse).
    pub after_item_list: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// A script whose game shows `available` free spots on My Listings page `FIRST_PAGE`.
pub fn script(available: Vec<i64>) -> Script {
    Script {
        now: 1000.0,
        snapshot: ListingsSnapshot { received_at: 1000.0, available, current_page: FIRST_PAGE, payouts: vec![] },
        outcomes: VecDeque::new(),
        confirm: true,
        answers_page: FIRST_PAGE,
        responsive: true,
        after_listing_page: None,
        turns_pages: true,
        shown_page: FIRST_PAGE,
        searches: VecDeque::new(),
        page_numbers: VecDeque::new(),
        shown_numbers: None,
        after_item_list: None,
    }
}

impl Script {
    pub fn build(self) -> Arc<ScriptedGame> {
        let shown_page = self.answers_page;
        Arc::new(ScriptedGame(Mutex::new(Script { shown_page, ..self })))
    }
}

pub fn searches(pages: Vec<Option<Vec<MarketRow>>>) -> VecDeque<Option<Vec<MarketRow>>> {
    pages.into()
}

pub struct ScriptedGame(Mutex<Script>);

impl ScriptedGame {
    pub fn set_now(&self, now: f64) {
        self.0.lock().unwrap().now = now;
    }

    pub fn set_responsive(&self, responsive: bool) {
        self.0.lock().unwrap().responsive = responsive;
    }
}

impl ClickListener for ScriptedGame {
    fn on_click(&self, clicked: ScreenPoint) {
        let mut script = self.0.lock().unwrap();
        if clicked == point("my_listings_tab") {
            script.shown_page = script.answers_page;
        } else if clicked == point("next_page_arrow") && script.turns_pages {
            script.shown_page += 1;
        } else if clicked == point("prev_page_arrow") && script.turns_pages {
            script.shown_page -= 1;
        }
    }
}

impl MarketplaceGame for ScriptedGame {
    fn now(&self) -> f64 {
        self.0.lock().unwrap().now
    }

    fn snapshot(&self) -> Option<ListingsSnapshot> {
        Some(self.0.lock().unwrap().snapshot.clone())
    }

    /// Stands in for the game re-sending My Listings when its tab is opened or a page turns.
    fn wait_for_fresh_snapshot(&self, since: f64, _timeout: f64) -> Option<ListingsSnapshot> {
        let mut script = self.0.lock().unwrap();
        if !script.responsive {
            return None;
        }
        script.snapshot = ListingsSnapshot { received_at: since + 0.001, current_page: script.shown_page, ..script.snapshot.clone() };
        Some(script.snapshot.clone())
    }

    fn wait_for_item_list(&self, _since: f64, _timeout: f64) -> Option<Vec<MarketRow>> {
        let (rows, hook) = {
            let mut script = self.0.lock().unwrap();
            let rows = script.searches.pop_front().flatten();
            if rows.as_ref().is_some_and(|rows| !rows.is_empty()) {
                if let Some(numbers) = script.page_numbers.pop_front() {
                    script.shown_numbers = Some(numbers);
                }
            }
            (rows, script.after_item_list.clone())
        };
        if let Some(hook) = hook {
            hook();
        }
        rows
    }

    fn last_item_page(&self) -> Option<(i64, i64)> {
        self.0.lock().unwrap().shown_numbers
    }

    fn begin_transfer(&self) {}

    fn wait_for_transfer(&self, _timeout: f64) -> Option<i64> {
        None
    }

    fn begin_register(&self) {}

    fn wait_for_register(&self, _timeout: f64) -> RegisterOutcome {
        self.0.lock().unwrap().outcomes.pop_front().unwrap_or(RegisterOutcome::Ok)
    }

    fn wait_for_listing(&self, _unique_id: &str, since: f64, _timeout: f64) -> bool {
        let mut script = self.0.lock().unwrap();
        if let Some(page) = script.after_listing_page.filter(|_| script.confirm) {
            script.shown_page = page;
            script.snapshot = ListingsSnapshot { received_at: since + 0.5, current_page: page, ..script.snapshot.clone() };
        }
        script.confirm
    }
}

/// A My Listings snapshot with one free spot (5) and `payouts` waiting (`_snap`).
pub fn payout_snapshot(payouts: Vec<(i64, i64, &str, i64)>) -> ListingsSnapshot {
    let payouts = payouts.into_iter().map(|(index, state, item, price)| (index, state, item.to_string(), price)).collect();
    ListingsSnapshot { received_at: 1000.0, available: vec![5], current_page: FIRST_PAGE, payouts }
}

/// Serves a scripted sequence of My Listings snapshots and transfer results (`PayoutState`).
pub struct PayoutGame {
    snapshots: Mutex<(ListingsSnapshot, VecDeque<ListingsSnapshot>)>,
    transfers: Mutex<VecDeque<i64>>,
    on_transfer: Mutex<Option<Box<dyn Fn() + Send>>>,
}

impl PayoutGame {
    /// The first snapshot is what the game showed before the run.
    pub fn new(snapshots: Vec<ListingsSnapshot>, transfers: Vec<i64>) -> Arc<Self> {
        let mut queue: VecDeque<ListingsSnapshot> = snapshots.into();
        let shown = queue.pop_front().expect("a first snapshot");
        Arc::new(PayoutGame { snapshots: Mutex::new((shown, queue)), transfers: Mutex::new(transfers.into()), on_transfer: Mutex::new(None) })
    }

    /// Runs `hook` while the runner waits for a transfer's answer.
    pub fn during_transfer(&self, hook: impl Fn() + Send + 'static) {
        *self.on_transfer.lock().unwrap() = Some(Box::new(hook));
    }
}

impl ClickListener for PayoutGame {
    fn on_click(&self, _point: ScreenPoint) {}
}

impl MarketplaceGame for PayoutGame {
    fn now(&self) -> f64 {
        1000.0
    }

    fn snapshot(&self) -> Option<ListingsSnapshot> {
        Some(self.snapshots.lock().unwrap().0.clone())
    }

    fn wait_for_fresh_snapshot(&self, since: f64, _timeout: f64) -> Option<ListingsSnapshot> {
        let mut snapshots = self.snapshots.lock().unwrap();
        let next = snapshots.1.pop_front()?;
        snapshots.0 = ListingsSnapshot { received_at: since + 0.001, ..next };
        Some(snapshots.0.clone())
    }

    fn wait_for_item_list(&self, _since: f64, _timeout: f64) -> Option<Vec<MarketRow>> {
        None
    }

    fn last_item_page(&self) -> Option<(i64, i64)> {
        None
    }

    fn begin_transfer(&self) {}

    fn wait_for_transfer(&self, _timeout: f64) -> Option<i64> {
        if let Some(hook) = self.on_transfer.lock().unwrap().as_ref() {
            hook();
        }
        self.transfers.lock().unwrap().pop_front()
    }

    fn begin_register(&self) {}

    fn wait_for_register(&self, _timeout: f64) -> RegisterOutcome {
        RegisterOutcome::Timeout
    }

    fn wait_for_listing(&self, _unique_id: &str, _since: f64, _timeout: f64) -> bool {
        false
    }
}

struct Paged {
    /// `(my_item_state, item_id, price)` in spot order.
    listings: Vec<(i64, String, i64)>,
    page: i64,
    spots: i64,
    selected: Option<usize>,
    shown: ListingsSnapshot,
}

impl Paged {
    fn page_snapshot(&self, received_at: f64) -> ListingsSnapshot {
        let first = self.page * 10;
        let payouts = (self.listings.iter().enumerate())
            .filter(|(i, (state, _, _))| (first..first + 10).contains(&(*i as i64)) && [2, 3].contains(state))
            .map(|(i, (state, item, price))| (i as i64, *state, item.clone(), *price))
            .collect();
        let available = (self.listings.len() as i64..self.spots).collect();
        ListingsSnapshot { received_at, available, current_page: FIRST_PAGE + self.page, payouts }
    }
}

/// My Listings across pages: the game shows one page at a time, and listings shift up when one is
/// collected (`PagedListingsGame`).
pub struct PagedListingsGame(Mutex<Paged>);

impl PagedListingsGame {
    pub fn new(listings: Vec<(i64, String, i64)>, shown_page: i64) -> Arc<Self> {
        let mut paged = Paged {
            listings,
            page: shown_page,
            spots: 30,
            selected: None,
            shown: ListingsSnapshot { received_at: 0.0, available: vec![], current_page: 0, payouts: vec![] },
        };
        paged.shown = paged.page_snapshot(1000.0);
        Arc::new(PagedListingsGame(Mutex::new(paged)))
    }

    pub fn listings(&self) -> Vec<(i64, String, i64)> {
        self.0.lock().unwrap().listings.clone()
    }
}

impl ClickListener for PagedListingsGame {
    fn on_click(&self, clicked: ScreenPoint) {
        let mut paged = self.0.lock().unwrap();
        let rows: Vec<ScreenPoint> = (0..10).map(|r| layout().spot_row(r)).collect();
        if clicked == point("next_page_arrow") {
            paged.page += 1;
        } else if clicked == point("prev_page_arrow") {
            paged.page -= 1;
        } else if let Some(row) = rows.iter().position(|p| *p == clicked) {
            paged.selected = Some(paged.page as usize * 10 + row);
        } else if clicked == point("transfer_all_button") {
            if let Some(selected) = paged.selected.take() {
                if [2, 3].contains(&paged.listings[selected].0) {
                    paged.listings.remove(selected);
                }
            }
        }
    }
}

impl MarketplaceGame for PagedListingsGame {
    fn now(&self) -> f64 {
        1000.0
    }

    fn snapshot(&self) -> Option<ListingsSnapshot> {
        Some(self.0.lock().unwrap().shown.clone())
    }

    fn wait_for_fresh_snapshot(&self, since: f64, _timeout: f64) -> Option<ListingsSnapshot> {
        let mut paged = self.0.lock().unwrap();
        paged.shown = paged.page_snapshot(since + 0.001);
        Some(paged.shown.clone())
    }

    fn wait_for_item_list(&self, _since: f64, _timeout: f64) -> Option<Vec<MarketRow>> {
        None
    }

    fn last_item_page(&self) -> Option<(i64, i64)> {
        None
    }

    fn begin_transfer(&self) {}

    fn wait_for_transfer(&self, _timeout: f64) -> Option<i64> {
        Some(1)
    }

    fn begin_register(&self) {}

    fn wait_for_register(&self, _timeout: f64) -> RegisterOutcome {
        RegisterOutcome::Timeout
    }

    fn wait_for_listing(&self, _unique_id: &str, _since: f64, _timeout: f64) -> bool {
        false
    }
}

/// A Marketplace runner on `game` whose clicks the game sees (`_runner`), with no pauses.
pub fn market_runner<G>(driver: &Arc<FakeDriver>, game: &Arc<G>, cancel: &CancelToken, safety: Option<Arc<dyn SafetyCheck>>) -> MarketplaceRunner
where
    G: MarketplaceGame + ClickListener + 'static,
{
    let listener = Arc::clone(game);
    driver.on_action(move |action, _| {
        if let Action::Click(clicked) = action {
            listener.on_click(*clicked);
        }
    });
    let runner =
        MarketplaceRunner::new(driver.clone(), layout().clone(), game.clone(), MAPPING.to_vec(), cancel.clone(), Box::new(|| {}));
    match safety {
        Some(safety) => runner.with_safety(safety),
        None => runner,
    }
}

/// The Marketplace tab order the merchant tests use.
pub const MERCHANT_MAPPING: [i32; 5] = [4, 5, 20, 21, 30];
pub const COLLECTOR: &str = "TheCollector";

/// A merchant sale entry (`_entry` in the Python merchant tests).
pub fn sale_entry(uid: &str, stash: &str, slot: i64, size: (i64, i64), vendor: i64, quantity: i64) -> PlanEntry {
    PlanEntry { quantity, ..PlanEntry::new(uid, format!("Item {uid}"), 3, stash, slot, size.0, size.1, 0, 0, vendor) }
}

/// A 1x1 item in stash 4 worth 10g.
pub fn item(uid: &str, slot: i64) -> PlanEntry {
    sale_entry(uid, "4", slot, (1, 1), 10, 1)
}

/// How the fake merchant behaves; build with struct update syntax over [`merchant`] (the last
/// four fields are its running state).
pub struct Merchant {
    /// Which entry sits under each stash point, for telling what was dragged.
    pub by_point: HashMap<ScreenPoint, String>,
    /// The merchant whose window opens when The Collector's card is clicked.
    pub opens: Option<String>,
    pub answers: bool,
    pub refuses: HashSet<String>,
    pub extra_sold: Vec<String>,
    pub result: i64,
    pub sells_nothing: bool,
    pub opened: Option<String>,
    pub staged: Vec<String>,
    pub reply: Option<SellBack>,
    pub deals: Vec<Vec<String>>,
}

/// A merchant that opens The Collector and sells whatever of `entries` is dragged into the box.
pub fn merchant(entries: &[PlanEntry]) -> Merchant {
    let by_point = entries
        .iter()
        .map(|e| (layout().item_centre(&e.stash_id, e.slot_id as i32, e.width as i32, e.height as i32), e.unique_id.clone()))
        .collect();
    Merchant {
        by_point,
        opens: Some(COLLECTOR.to_string()),
        answers: true,
        refuses: HashSet::new(),
        extra_sold: vec![],
        result: SELL_SUCCESS,
        sells_nothing: false,
        opened: None,
        staged: vec![],
        reply: None,
        deals: vec![],
    }
}

impl Merchant {
    pub fn build(self) -> Arc<FakeMerchant> {
        Arc::new(FakeMerchant(Mutex::new(self)))
    }
}

/// Opens the merchant whose card is clicked and sells what was dragged into the Sell box
/// (`FakeGame`).
pub struct FakeMerchant(Mutex<Merchant>);

impl FakeMerchant {
    /// Unique ids sold by each Make Deal.
    pub fn deals(&self) -> Vec<Vec<String>> {
        self.0.lock().unwrap().deals.clone()
    }

    fn on_action(&self, action: &Action) {
        let mut game = self.0.lock().unwrap();
        match action {
            Action::Click(clicked) if *clicked == layout().merchant_card(MERCHANT_CARD_INDEX) => {
                game.opened = game.opens.clone();
            }
            Action::Drag(from, _) => {
                if let Some(uid) = game.by_point.get(from).cloned().filter(|uid| !game.refuses.contains(uid)) {
                    game.staged.push(uid);
                }
            }
            Action::Click(clicked) if *clicked == point("merchant_make_deal") => {
                let mut sold: Vec<String> = Vec::new();
                if game.result == SELL_SUCCESS && !game.sells_nothing {
                    sold = game.staged.iter().chain(&game.extra_sold).cloned().collect();
                }
                game.deals.push(sold.clone());
                game.reply = game.answers.then(|| SellBack { received_at: 51.0, result: game.result, deleted_ids: sold });
                game.staged.clear();
            }
            Action::Escape => game.staged.clear(),
            _ => {}
        }
    }
}

impl MerchantGame for FakeMerchant {
    fn now(&self) -> f64 {
        50.0
    }

    fn wait_for_merchant(&self, key: &str, _since: f64, _timeout: f64) -> bool {
        self.0.lock().unwrap().opened.as_deref() == Some(key)
    }

    fn wait_for_sell_back(&self, _since: f64, _timeout: f64) -> Option<SellBack> {
        self.0.lock().unwrap().reply.take()
    }
}

/// Sells `entries` through a runner on `game` (`_run`), checking every result was also reported as
/// progress, in order.
pub fn sell(
    entries: &[PlanEntry],
    game: &Arc<FakeMerchant>,
    driver: &Arc<FakeDriver>,
    dry_run: bool,
    cancel: &CancelToken,
    safety: Option<Arc<dyn SafetyCheck>>,
) -> RunReport {
    let listener = Arc::clone(game);
    driver.on_action(move |action, _| listener.on_action(action));
    let runner = MerchantSaleRunner::new(
        driver.clone(),
        layout().clone(),
        game.clone(),
        MERCHANT_MAPPING.to_vec(),
        cancel.clone(),
        Box::new(|| {}),
    );
    let runner = match safety {
        Some(safety) => runner.with_safety(safety),
        None => runner,
    };
    let mut progress = Vec::new();
    let report = lister::job::MerchantRunner::sell(&runner, entries, dry_run, &mut |r| progress.push(r));
    let reported: Vec<&str> = progress.iter().map(|r| r.unique_id.as_str()).collect();
    let results: Vec<&str> = report.results.iter().map(|r| r.unique_id.as_str()).collect();
    assert_eq!(reported, results);
    report
}

/// The desktop the safety monitor watches: a cursor and focus the test sets, counting reads.
#[derive(Default)]
pub struct FakeDesktop {
    cursor: Mutex<Option<ScreenPoint>>,
    unfocused: std::sync::atomic::AtomicBool,
    cursor_reads: AtomicUsize,
    focus_polls: AtomicUsize,
}

impl FakeDesktop {
    /// A focused game with the cursor at `(0, 0)`.
    pub fn new() -> Arc<Self> {
        let desktop = FakeDesktop::default();
        desktop.set_cursor(Some((0, 0)));
        Arc::new(desktop)
    }

    pub fn set_cursor(&self, cursor: Option<ScreenPoint>) {
        *self.cursor.lock().unwrap() = cursor;
    }

    pub fn set_focused(&self, focused: bool) {
        self.unfocused.store(!focused, Ordering::SeqCst);
    }

    pub fn cursor_reads(&self) -> usize {
        self.cursor_reads.load(Ordering::SeqCst)
    }

    pub fn focus_polls(&self) -> usize {
        self.focus_polls.load(Ordering::SeqCst)
    }
}

impl lister::runner::Desktop for FakeDesktop {
    fn cursor_position(&self) -> Option<ScreenPoint> {
        self.cursor_reads.fetch_add(1, Ordering::SeqCst);
        *self.cursor.lock().unwrap()
    }

    fn game_has_focus(&self) -> bool {
        self.focus_polls.fetch_add(1, Ordering::SeqCst);
        !self.unfocused.load(Ordering::SeqCst)
    }
}

/// A game window that is brought forward (with the given client area) or not.
pub struct FakeWindow {
    answer: Result<input::WindowRect, String>,
    calls: AtomicUsize,
}

impl FakeWindow {
    pub fn at(left: i32, top: i32, width: i32, height: i32) -> Arc<Self> {
        Arc::new(FakeWindow { answer: Ok(input::WindowRect { left, top, width, height }), calls: AtomicUsize::new(0) })
    }

    pub fn failing(message: &str) -> Arc<Self> {
        Arc::new(FakeWindow { answer: Err(message.to_string()), calls: AtomicUsize::new(0) })
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl lister::runner::GameWindow for FakeWindow {
    fn bring_forward(&self, _cancel: &CancelToken) -> Result<input::WindowRect, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.answer.clone()
    }
}

/// An environment of fakes with no waiting.
pub fn fake_environment(
    window: &Arc<FakeWindow>,
    driver: &Arc<FakeDriver>,
    desktop: &Arc<FakeDesktop>,
) -> lister::runner::Environment {
    lister::runner::Environment {
        window: window.clone(),
        input: driver.clone(),
        desktop: desktop.clone(),
        sleep: Arc::new(|_: &CancelToken, _: std::time::Duration| {}),
    }
}

/// A re-check before listing from a closure (the Python tests' `reprice=lambda entry, market: ...`).
pub struct Recheck<F>(pub F);

impl<F> lister::job::Reprice for Recheck<F>
where
    F: Fn(&PlanEntry, &lister::plan::MarketBucket) -> lister::job::RepriceDecision + Send + Sync,
{
    fn reprice(&self, entry: &PlanEntry, market: &lister::plan::MarketBucket) -> lister::job::RepriceDecision {
        (self.0)(entry, market)
    }
}

pub fn decision(price: Option<i64>, note: &str) -> lister::job::RepriceDecision {
    lister::job::RepriceDecision { price, note: note.to_string() }
}

/// Collects progress reports.
pub fn progress_log() -> (Arc<Mutex<Vec<ItemResult>>>, impl FnMut(ItemResult)) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&log);
    (log, move |result| sink.lock().unwrap().push(result))
}
