import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
	addLibraryFolder,
	cancelLibraryScan,
	libraryStatus,
	queryLibrary,
	removeLibraryFolder,
	rescanLibraryFolder,
	scanActive,
	type LibraryPage,
	type LibraryQuery,
	type LibraryStatus,
} from "../services/library";
import { errorMessage } from "../services/media";
import LibraryBrowser, { type LibraryView } from "./LibraryBrowser";
import LibraryTools from "./LibraryTools";
import styles from "./Library.module.scss";

const initialQuery: LibraryQuery = {
	search: "",
	rootId: null,
	sort: "date",
	descending: true,
	filter: "all",
	codec: "all",
	offset: 0,
};
const viewKey = "klipper.libraryView";
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
}: {
	active: boolean;
	selecting: boolean;
	onOpen: (id: number) => void;
}) {
	const [status, setStatus] = useState<LibraryStatus | null>(null);
	const [page, setPage] = useState<LibraryPage>({ items: [], total: 0, limit: 40 });
	const [loadedQuery, setLoadedQuery] = useState("");
	const [query, setQuery] = useState(initialQuery);
	const [search, setSearch] = useState("");
	const [view, setView] = useState<LibraryView>(preferredView);
	const [selectedId, setSelectedId] = useState<number | null>(null);
	const [revision, setRevision] = useState(0);
	const [busy, setBusy] = useState(false);
	const [loading, setLoading] = useState(true);
	const [error, setError] = useState("");
	const [dismissedScan, setDismissedScan] = useState("");
	const list = useRef<HTMLDivElement>(null);
	const scroll = useRef(0);
	const previousActive = useRef(active);
	const alive = useRef(false);
	const actionPending = useRef(false);
	const scanning = scanActive(status?.progress.phase ?? "");
	const queryPending = loadedQuery !== JSON.stringify(query);

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
		let timer: ReturnType<typeof setTimeout>;
		async function load() {
			let delay = 2500;
			try {
				const [overview, result] = await Promise.all([
					libraryStatus(),
					queryLibrary(query),
				]);
				if (!disposed) {
					setStatus(overview);
					setPage(result);
					setLoadedQuery(JSON.stringify(query));
					delay = scanActive(overview.progress.phase) ? 600 : 2500;
					if (query.offset > 0 && query.offset >= result.total) {
						setQuery((previous) => ({ ...previous, offset: 0 }));
					}
				}
			} catch (failure) {
				if (!disposed) setError(errorMessage(failure));
			} finally {
				if (!disposed) {
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

	function resetScroll() {
		scroll.current = 0;
		if (list.current) list.current.scrollTop = 0;
	}
	function filter(changes: Partial<LibraryQuery>) {
		setQuery((previous) => ({ ...previous, ...changes, offset: 0 }));
		resetScroll();
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
		!!search || query.rootId !== null || query.filter !== "all" || query.codec !== "all";

	return (
		<section className={styles.library} hidden={!active} aria-label="Recording library">
			<LibraryTools
				query={query}
				search={search}
				view={view}
				status={status}
				count={page.total}
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
						if (alive.current && query.rootId === id) filter({ rootId: null });
					})
				}
			/>
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
			{!loading && !page.items.length ? (
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
						{status?.roots.length
							? "No matching recordings"
							: "Your recordings start here"}
					</h2>
					<p>
						{status?.roots.length
							? "Try a different search or filter, or rescan a folder to find new recordings."
							: "Add a local folder to browse your recordings. Your source files stay untouched."}
					</p>
					{filtered && (
						<button
							onClick={() => {
								setSearch("");
								filter({ search: "", rootId: null, filter: "all", codec: "all" });
							}}
						>
							Clear search and filters
						</button>
					)}
				</div>
			) : (
				<LibraryBrowser
					items={page.items}
					view={view}
					root={list}
					active={active && !queryPending}
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
				/>
			)}
			{loading && (
				<span className={styles.loading} role="status">
					Loading recordings…
				</span>
			)}
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
		</section>
	);
}
