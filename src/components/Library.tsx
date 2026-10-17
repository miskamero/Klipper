import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
	addLibraryFolder,
	cancelLibraryScan,
	libraryStatus,
	queryLibrary,
	removeLibraryFolder,
	rescanLibraryFolder,
	resetLibrary,
	scanActive,
	type LibraryPage,
	type LibraryQuery,
	type LibraryStatus,
} from "../services/library";
import { errorMessage } from "../services/media";
import LibraryBrowser, { type LibraryView } from "./LibraryBrowser";
import LibraryTools from "./LibraryTools";
import MedalImport from "./MedalImport";
import LibrarySidebar, { sidebarKey } from "./LibrarySidebar";
import ResetLibrary from "./ResetLibrary";
import { thumbnails } from "../services/thumbnails";
import styles from "./Library.module.scss";
import { CollectionsHome, CollectionLocation } from "./LibraryCollections";
import collectionStyles from "./LibraryCollections.module.scss";

const initialQuery: LibraryQuery = {
	home: true,
	unknownGame: false,
	search: "",
	rootId: null,
	folderId: null,
	provider: null,
	sort: "date",
	descending: true,
	filter: "all",
	codec: "all",
	offset: 0,
};
const viewKey = "klipper.libraryView";
const browsingKey = "klipper.libraryBrowsing";
function preferredQuery(): LibraryQuery {
	try {
		const value = JSON.parse(localStorage.getItem(browsingKey) || "null");
		if (!value) return initialQuery;
		const rootId = Number.isSafeInteger(value.rootId) && value.rootId > 0 ? value.rootId : null;
		const folderId =
			typeof value.folderId === "string" && value.folderId.length < 100
				? value.folderId
				: null;
		const provider =
			typeof value.provider === "string" && /^[a-z0-9-]{1,64}$/.test(value.provider)
				? value.provider
				: null;
		return {
			...initialQuery,
			home:
				typeof value.home === "boolean"
					? value.home
					: !(rootId || folderId || provider || value.search || value.offset),
			unknownGame: value.unknownGame === true,
			search: typeof value.search === "string" ? value.search.slice(0, 200) : "",
			rootId,
			folderId,
			provider,
			sort: ["date", "name", "size", "duration"].includes(value.sort) ? value.sort : "date",
			descending: value.descending !== false,
			filter: ["all", "available", "missing", "pending", "error"].includes(value.filter)
				? value.filter
				: "all",
			codec: ["all", "h264", "hevc"].includes(value.codec) ? value.codec : "all",
			game:
				value.unknownGame !== true && typeof value.game === "string"
					? value.game.slice(0, 512)
					: null,
			offset:
				Number.isSafeInteger(value.offset) &&
				value.offset >= 0 &&
				value.offset <= 10_000_000
					? value.offset
					: 0,
		};
	} catch {
		return initialQuery;
	}
}
function preferredView(): LibraryView {
	try {
		return localStorage.getItem(viewKey) === "list" ? "list" : "grid";
	} catch {
		return "grid";
	}
}

export default function Library({
	active,
	selecting,
	onOpen,
	onReset,
}: {
	active: boolean;
	selecting: boolean;
	onOpen: (id: number) => void;
	onReset?: () => void;
}) {
	const [status, setStatus] = useState<LibraryStatus | null>(null);
	const [page, setPage] = useState<LibraryPage>({ items: [], total: 0, limit: 40 });
	const [loadedQuery, setLoadedQuery] = useState("");
	const [query, setQuery] = useState(preferredQuery);
	const [search, setSearch] = useState(query.search);
	const [view, setView] = useState<LibraryView>(preferredView);
	const [selectedId, setSelectedId] = useState<number | null>(null);
	const [revision, setRevision] = useState(0);
	const [busy, setBusy] = useState(false);
	const [loading, setLoading] = useState(true);
	const [error, setError] = useState("");
	const [importing, setImporting] = useState(false);
	const [confirmReset, setConfirmReset] = useState(false);
	const [resetting, setResetting] = useState(false);
	const [resetSerial, setResetSerial] = useState(0);
	const loadEpoch = useRef(0);
	const [dismissedScan, setDismissedScan] = useState("");
	const list = useRef<HTMLDivElement>(null);
	const scroll = useRef(0);
	const previousActive = useRef(active);
	const alive = useRef(false);
	const actionPending = useRef(false);
	const scanning = scanActive(status?.progress.phase ?? "");
	const queryPending = loadedQuery !== JSON.stringify(query);
	const home = query.home === true;
	const navigation = status?.navigation ?? { total: page.total, sources: [], folders: [] };
	const currentFolder =
		navigation.folders.find((folder) =>
			query.folderId
				? folder.id === query.folderId
				: query.rootId !== null &&
					folder.rootId === query.rootId &&
					folder.parentId === null,
		) ?? null;

	useEffect(() => {
		alive.current = true;
		return () => {
			alive.current = false;
		};
	}, []);

	useEffect(() => {
		try {
			localStorage.setItem(viewKey, view);
		} catch {
			/* Preferences are optional. */
		}
	}, [view]);
	useEffect(() => {
		try {
			localStorage.setItem(browsingKey, JSON.stringify(query));
		} catch {
			/* Optional preferences. */
		}
	}, [query]);

	useLayoutEffect(() => {
		if (active && !previousActive.current && list.current) {
			list.current.scrollTop = scroll.current;
			if (selectedId !== null) {
				list.current
					.querySelector<HTMLButtonElement>(
						'button[data-recording-id="' + selectedId + '"]',
					)
					?.focus({ preventScroll: true });
			}
		}
		previousActive.current = active;
	}, [active, selectedId]);

	useEffect(() => {
		const timer = setTimeout(() => {
			setQuery((previous) =>
				previous.search === search ? previous : { ...previous, search, offset: 0 },
			);
		}, 200);
		return () => clearTimeout(timer);
	}, [search]);

	useEffect(() => {
		if (!active) return;
		let disposed = false;
		const epoch = loadEpoch.current;
		let timer: ReturnType<typeof setTimeout>;
		async function load() {
			let delay = 2500;
			try {
				const overview = await libraryStatus();
				if (disposed || epoch !== loadEpoch.current) return;
				const navigation = overview.navigation;
				if (
					(query.rootId !== null &&
						!overview.roots.some((root) => root.id === query.rootId)) ||
					(query.provider &&
						navigation &&
						!navigation.sources.some((source) => source.kind === query.provider)) ||
					(query.folderId &&
						navigation &&
						!navigation.folders.some((folder) => folder.id === query.folderId))
				) {
					setStatus(overview);
					setQuery((previous) => ({
						...previous,
						rootId: null,
						folderId: null,
						provider: null,
						home: true,
						offset: 0,
					}));
					scroll.current = 0;
					if (list.current) list.current.scrollTop = 0;
					return;
				}
				const result = query.home
					? { items: [], total: 0, limit: 40 }
					: await queryLibrary(query);
				if (!disposed && epoch === loadEpoch.current) {
					setStatus(overview);
					setPage(result);
					setLoadedQuery(JSON.stringify(query));
					delay = scanActive(overview.progress.phase) ? 600 : 2500;
					if (query.offset > 0 && query.offset >= result.total) {
						setQuery((previous) => ({ ...previous, offset: 0 }));
					}
				}
			} catch (failure) {
				if (!disposed && epoch === loadEpoch.current) setError(errorMessage(failure));
			} finally {
				if (!disposed && epoch === loadEpoch.current) {
					setLoading(false);
					timer = setTimeout(() => void load(), delay);
				}
			}
		}
		void load();
		return () => {
			disposed = true;
			clearTimeout(timer);
		};
	}, [active, query, revision]);

	async function reset() {
		if (actionPending.current) return;
		actionPending.current = true;
		loadEpoch.current++;
		setResetting(true);
		setBusy(true);
		setError("");
		try {
			const result = await resetLibrary();
			// Only forget browser state after the native transaction has committed.
			thumbnails.clear();
			try {
				for (const key of [viewKey, browsingKey, sidebarKey]) localStorage.removeItem(key);
			} catch {
				/* Optional preferences. */
			}
			if (!alive.current) return;
			onReset?.();
			setQuery(initialQuery);
			setSearch("");
			setView("grid");
			setSelectedId(null);
			setPage({ items: [], total: 0, limit: 40 });
			setStatus(null);
			setDismissedScan("");
			setImporting(false);
			resetScroll();
			setResetSerial((value) => value + 1);
			if (result.cacheWarning) setError(result.cacheWarning);
		} catch (failure) {
			if (alive.current) setError(errorMessage(failure));
		} finally {
			actionPending.current = false;
			if (alive.current) {
				setConfirmReset(false);
				setResetting(false);
				setBusy(false);
				setRevision((value) => value + 1);
			}
		}
	}

	function resetScroll() {
		scroll.current = 0;
		if (list.current) list.current.scrollTop = 0;
	}
	function filter(changes: Partial<LibraryQuery>) {
		setQuery((previous) => ({ ...previous, ...changes, offset: 0 }));
		resetScroll();
	}
	function selectCollection(changes: Partial<LibraryQuery>) {
		setSelectedId(null);
		filter({
			rootId: null,
			folderId: null,
			provider: null,
			...changes,
			home: false,
		});
	}
	function collectionsHome() {
		setSelectedId(null);
		filter({ home: true, rootId: null, folderId: null, provider: null });
	}
	function clearFilters() {
		setSearch("");
		filter({ search: "", filter: "all", codec: "all", game: null, unknownGame: false });
	}
	function changePage(offset: number) {
		setQuery((previous) => ({ ...previous, offset }));
		resetScroll();
	}
	async function action(run: () => Promise<unknown>) {
		if (actionPending.current) return;
		actionPending.current = true;
		setBusy(true);
		setError("");
		try {
			await run();
		} catch (failure) {
			if (alive.current) setError(errorMessage(failure));
		} finally {
			actionPending.current = false;
			if (alive.current) {
				setBusy(false);
				setRevision((value) => value + 1);
			}
		}
	}

	const progress = status?.progress;
	const scanKey = JSON.stringify([progress, status?.roots.map((root) => root.lastScan)]);
	const showScan =
		progress?.phase &&
		(scanning ||
			(dismissedScan !== scanKey &&
				(progress.issueCount > 0 || ["failed", "cancelled"].includes(progress.phase))));
	const filtered =
		!!search ||
		query.filter !== "all" ||
		query.codec !== "all" ||
		!!query.game ||
		!!query.unknownGame;

	return (
		<section className={styles.library} hidden={!active} aria-label="Recording library">
			<LibrarySidebar
				key={resetSerial}
				navigation={navigation}
				query={query}
				home={home}
				onHome={collectionsHome}
				onSelect={selectCollection}
			/>
			<div className={styles.content}>
				{!home && (
					<CollectionLocation
						current={currentFolder}
						provider={query.provider ?? null}
						folders={navigation.folders}
						onHome={collectionsHome}
						onSelect={selectCollection}
					/>
				)}
				<LibraryTools
					key={resetSerial}
					query={query}
					search={search}
					view={view}
					status={status}
					count={home ? navigation.total : page.total}
					home={home}
					games={page.games ?? []}
					unknownGameAvailable={page.unknownGameAvailable ?? false}
					onImport={() => setImporting(true)}
					onReset={() => setConfirmReset(true)}
					busy={busy}
					scanning={scanning}
					onSearch={(value) => {
						setSearch(value);
						resetScroll();
					}}
					onFilter={filter}
					onView={(next) => {
						setView(next);
						resetScroll();
					}}
					onAdd={() => void action(addLibraryFolder)}
					onRescan={(id) => void action(() => rescanLibraryFolder(id))}
					onRemove={(id) =>
						void action(async () => {
							await removeLibraryFolder(id);
							if (alive.current && query.rootId === id) collectionsHome();
						})
					}
				/>
				{confirmReset && (
					<ResetLibrary
						busy={resetting}
						onCancel={() => setConfirmReset(false)}
						onConfirm={() => void reset()}
					/>
				)}
				{active && importing && (
					<MedalImport
						onClose={() => setImporting(false)}
						onImported={() => setRevision((value) => value + 1)}
					/>
				)}
				{error && (
					<p className={styles.error} role="alert">
						{error}
					</p>
				)}
				{showScan && (
					<div className={styles.progress} role="status" aria-live="polite">
						<div>
							<strong>
								{scanning
									? progress.phase === "discovering"
										? "Discovering recordings"
										: "Inspecting recordings"
									: "Scan " + progress.phase}
							</strong>
							<span>
								{progress.discovered} found · {progress.analyzed} analyzed ·{" "}
								{progress.reused} unchanged
								{progress.skippedLinks
									? " · " + progress.skippedLinks + " links skipped"
									: ""}
							</span>
						</div>
						{!!progress.issueCount && (
							<details>
								<summary>{progress.issueCount} scan issues</summary>
								<ul>
									{progress.issues.map((issue, index) => (
										<li key={index}>{issue}</li>
									))}
								</ul>
								{progress.issueCount > progress.issues.length && (
									<p>Showing the first {progress.issues.length} issues.</p>
								)}
							</details>
						)}
						{scanning ? (
							<button disabled={busy} onClick={() => void action(cancelLibraryScan)}>
								Cancel scan
							</button>
						) : (
							<button
								aria-label="Dismiss scan status"
								onClick={() => setDismissedScan(scanKey)}
							>
								Dismiss
							</button>
						)}
					</div>
				)}
				{home ? (
					<CollectionsHome navigation={navigation} onSelect={selectCollection} />
				) : !loading && !page.items.length && !currentFolder ? (
					<div className={styles.empty}>
						<svg
							viewBox="0 0 24 24"
							fill="none"
							stroke="currentColor"
							strokeWidth="1.25"
							aria-hidden="true"
						>
							<rect x="3" y="5" width="18" height="14" rx="2" />
							<path d="m10 9 5 3-5 3Z" />
						</svg>
						<h2>
							{filtered || status?.roots.length || page.games?.length
								? "No matching recordings"
								: "Your recordings start here"}
						</h2>
						<p>
							{filtered || status?.roots.length || page.games?.length
								? "Try a different search or filter, or rescan a folder to find new recordings."
								: "Add a local folder or import Medal recordings to start browsing."}
						</p>
						{filtered && (
							<button onClick={clearFilters}>Clear search and filters</button>
						)}
					</div>
				) : (
					<LibraryBrowser
						items={page.items}
						view={view}
						root={list}
						active={active && !queryPending && !resetting}
						loading={loading || queryPending}
						selecting={selecting || queryPending}
						selectedId={selectedId}
						onOpen={(id) => {
							setSelectedId(id);
							onOpen(id);
						}}
						onScroll={() => {
							if (active && list.current) scroll.current = list.current.scrollTop;
						}}
					>
						{currentFolder && (
							<>
								<h2 className={collectionStyles.recordingsHeading}>
									Recordings in this folder and subfolders
								</h2>
								<p className={collectionStyles.caption}>
									Search and filters include all indexed subfolders.
								</p>
								{!loading && !queryPending && !page.items.length && (
									<p>
										{filtered
											? "No matching recordings in this folder."
											: "No indexed recordings in this folder or its subfolders."}
									</p>
								)}
								{!loading && !queryPending && !page.items.length && filtered && (
									<button onClick={clearFilters}>Clear search and filters</button>
								)}
							</>
						)}
					</LibraryBrowser>
				)}
				{loading && (
					<span className={styles.loading} role="status">
						Loading recordings…
					</span>
				)}
				{!home && (
					<footer className={styles.footer}>
						<span>
							{page.total > 0
								? query.offset +
									1 +
									"–" +
									Math.min(query.offset + page.limit, page.total) +
									" of " +
									page.total.toLocaleString() +
									" recordings"
								: "No recordings"}
						</span>
						<div aria-label="Library pages">
							<button
								disabled={query.offset === 0}
								onClick={() => changePage(Math.max(0, query.offset - page.limit))}
							>
								Previous
							</button>
							<span>
								Page {Math.floor(query.offset / page.limit) + 1} of{" "}
								{Math.max(1, Math.ceil(page.total / page.limit))}
							</span>
							<button
								disabled={query.offset + page.limit >= page.total}
								onClick={() => changePage(query.offset + page.limit)}
							>
								Next
							</button>
						</div>
					</footer>
				)}
			</div>
		</section>
	);
}
