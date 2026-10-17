import { useEffect, useRef, useState } from "react";
import type { LibraryFolder, LibraryNavigation, LibraryQuery } from "../services/library";
import styles from "./LibrarySidebar.module.scss";

export const sidebarKey = "klipper.librarySidebar";
interface Preferences {
	collapsed: boolean;
	sources: boolean;
	folders: boolean;
	expanded: string[];
}
function preferences(): Preferences {
	try {
		const value = JSON.parse(localStorage.getItem(sidebarKey) || "null");
		return {
			collapsed: value?.collapsed !== false,
			sources: value?.sources !== false,
			folders: value?.folders !== false,
			expanded: Array.isArray(value?.expanded)
				? value.expanded.filter((id: unknown) => typeof id === "string").slice(0, 5000)
				: [],
		};
	} catch {
		return { collapsed: true, sources: true, folders: true, expanded: [] };
	}
}

function Icon({ folder = false }: { folder?: boolean }) {
	return (
		<svg
			viewBox="0 0 18 18"
			fill="none"
			stroke="currentColor"
			strokeWidth="1.3"
			aria-hidden="true"
		>
			{folder ? (
				<path d="M2 5V3h5l2 2h7v10H2z" />
			) : (
				<>
					<rect x="2" y="3" width="14" height="12" rx="1" />
					<path d="m7 6 5 3-5 3z" />
				</>
			)}
		</svg>
	);
}

export default function LibrarySidebar({
	navigation,
	query,
	onSelect,
	home,
	onHome,
}: {
	navigation: LibraryNavigation;
	query: LibraryQuery;
	onSelect: (changes: Partial<LibraryQuery>) => void;
	home: boolean;
	onHome: () => void;
}) {
	const [prefs, setPrefs] = useState(preferences);
	const selection = `${query.rootId ?? ""}/${query.folderId ?? ""}`;
	const previousSelection = useRef(selection);
	useEffect(() => {
		// Reveal main-area navigation in the tree, without undoing a saved/manual collapse.
		if (previousSelection.current === selection) return;
		previousSelection.current = selection;
		const ancestors: string[] = [];
		let folder = navigation.folders.find((item) => item.id === query.folderId);
		while (folder?.parentId && ancestors.length < navigation.folders.length) {
			ancestors.push(folder.parentId);
			folder = navigation.folders.find((item) => item.id === folder?.parentId);
		}
		if (ancestors.length)
			setPrefs((previous) =>
				ancestors.every((id) => previous.expanded.includes(id))
					? previous
					: { ...previous, expanded: [...new Set([...previous.expanded, ...ancestors])] },
			);
	}, [selection, query.folderId, navigation.folders]);
	useEffect(() => {
		try {
			localStorage.setItem(sidebarKey, JSON.stringify(prefs));
		} catch {
			/* Optional preferences. */
		}
	}, [prefs]);
	function choose(changes: Partial<LibraryQuery>) {
		onSelect({ rootId: null, folderId: null, provider: null, ...changes });
	}
	const childrenByParent = new Map<string | null, LibraryFolder[]>();
	for (const folder of navigation.folders) {
		const children = childrenByParent.get(folder.parentId) ?? [];
		children.push(folder);
		childrenByParent.set(folder.parentId, children);
	}
	function node(folder: LibraryFolder, depth = 0) {
		const children = childrenByParent.get(folder.id) ?? [];
		const open = prefs.expanded.includes(folder.id);
		const selected = folder.parentId
			? query.folderId === folder.id
			: query.rootId === folder.rootId && !query.folderId;
		return (
			<li key={folder.id}>
				<div className={styles.folder} style={{ paddingLeft: Math.min(depth, 4) * 8 }}>
					<button
						className={styles.expander}
						disabled={!children.length}
						aria-label={(open ? "Collapse " : "Expand ") + folder.name}
						aria-expanded={children.length ? open : undefined}
						onClick={() =>
							setPrefs((p) => ({
								...p,
								expanded: open
									? p.expanded.filter((id) => id !== folder.id)
									: [...p.expanded, folder.id],
							}))
						}
					>
						{children.length ? (open ? "▾" : "▸") : ""}
					</button>
					<button
						className={styles.collection}
						aria-current={selected ? "page" : undefined}
						title={folder.path}
						onClick={() =>
							choose({
								rootId: folder.rootId,
								folderId: folder.parentId ? folder.id : null,
							})
						}
					>
						<Icon folder />
						<span>{folder.name}</span>
						<small>{folder.recordings.toLocaleString()}</small>
					</button>
				</div>
				{open && children.length > 0 && (
					<ul>{children.map((child) => node(child, depth + 1))}</ul>
				)}
			</li>
		);
	}
	return (
		<aside
			className={`${styles.sidebar} ${prefs.collapsed ? styles.collapsed : ""}`}
			aria-label="Library collections"
		>
			<button
				className={styles.toggle}
				aria-label={prefs.collapsed ? "Show library sidebar" : "Hide library sidebar"}
				aria-expanded={!prefs.collapsed}
				onClick={() => setPrefs((p) => ({ ...p, collapsed: !p.collapsed }))}
			>
				☰{!prefs.collapsed && <span>Collections</span>}
			</button>
			{!prefs.collapsed && (
				<nav aria-label="Library collections">
					<button
						className={styles.collection}
						aria-current={home ? "page" : undefined}
						onClick={onHome}
					>
						Collections
					</button>
					<button
						className={styles.collection}
						aria-current={
							!home && !query.rootId && !query.provider ? "page" : undefined
						}
						onClick={() => choose({})}
					>
						<Icon />
						<span>All recordings</span>
						<small>{navigation.total.toLocaleString()}</small>
					</button>
					{navigation.sources.length > 0 && (
						<section>
							<button
								className={styles.section}
								aria-expanded={prefs.sources}
								onClick={() => setPrefs((p) => ({ ...p, sources: !p.sources }))}
							>
								{prefs.sources ? "▾" : "▸"} Sources
							</button>
							{prefs.sources &&
								navigation.sources.map((source) => (
									<button
										key={source.kind}
										className={styles.collection}
										aria-current={
											query.provider === source.kind ? "page" : undefined
										}
										onClick={() => choose({ provider: source.kind })}
									>
										<Icon />
										<span>
											{source.kind === "medal" ? "Medal" : source.kind}
										</span>
										<small>{source.recordings.toLocaleString()}</small>
									</button>
								))}
						</section>
					)}
					<section>
						<button
							className={styles.section}
							aria-expanded={prefs.folders}
							onClick={() => setPrefs((p) => ({ ...p, folders: !p.folders }))}
						>
							{prefs.folders ? "▾" : "▸"} Folders
						</button>
						{prefs.folders && (
							<ul>
								{(childrenByParent.get(null) ?? []).map((folder) => node(folder))}
							</ul>
						)}
						{prefs.folders && !navigation.folders.length && <p>No indexed folders</p>}
					</section>
				</nav>
			)}
		</aside>
	);
}
