import type { RefObject, ReactNode } from "react";
import { bytes, time } from "../lib/format";
import type { LibraryRecording } from "../services/library";
import Thumbnail from "./Thumbnail";
import styles from "./Library.module.scss";

export type LibraryView = "grid" | "list";

function modifiedDate(value: number | null) {
	return value === null
		? "Date unavailable"
		: new Date(value).toLocaleDateString(undefined, {
				month: "short",
				day: "numeric",
				year: "numeric",
			});
}

function videoLabel(item: LibraryRecording) {
	const codec = item.videoCodec === "h264" ? "H.264" : item.videoCodec?.toUpperCase();
	return (
		[item.width && item.height ? item.width + " × " + item.height : null, codec]
			.filter(Boolean)
			.join(" · ") ||
		(item.status === "pending" ? "Awaiting inspection" : "Video details unavailable")
	);
}

export default function LibraryBrowser({
	items,
	view,
	root,
	active,
	loading,
	selecting,
	selectedId,
	onOpen,
	onScroll,
	children,
}: {
	items: LibraryRecording[];
	view: LibraryView;
	root: RefObject<HTMLDivElement | null>;
	active: boolean;
	loading: boolean;
	selecting: boolean;
	selectedId: number | null;
	onOpen: (id: number) => void;
	onScroll: () => void;
	children?: ReactNode;
}) {
	return (
		<div
			ref={root}
			className={styles.browser}
			data-view={view}
			aria-label="Library recordings"
			aria-busy={loading}
			tabIndex={0}
			onScroll={onScroll}
			onKeyDown={(event) => {
				const target = event.target;
				if (!(target instanceof HTMLButtonElement) || !target.dataset.recordingId) return;
				const buttons = [
					...event.currentTarget.querySelectorAll<HTMLButtonElement>(
						"button[data-recording-id]",
					),
				];
				const index = buttons.indexOf(target);
				const collection = event.currentTarget.querySelector("ul");
				const columns =
					view === "grid" && collection
						? Math.max(
								1,
								getComputedStyle(collection)
									.gridTemplateColumns.split(" ")
									.filter(Boolean).length,
							)
						: 1;
				let next: number;
				switch (event.key) {
					case "ArrowRight":
						next = index + 1;
						break;
					case "ArrowLeft":
						next = index - 1;
						break;
					case "ArrowDown":
						next = index + columns;
						break;
					case "ArrowUp":
						next = index - columns;
						break;
					case "Home":
						next = 0;
						break;
					case "End":
						next = buttons.length - 1;
						break;
					default:
						return;
				}
				event.preventDefault();
				const direction = next < index ? -1 : 1;
				while (next >= 0 && next < buttons.length && buttons[next].disabled)
					next += direction;
				buttons[next]?.focus();
			}}
		>
			{children}
			{view === "list" && items.length > 0 && (
				<div className={styles.columnHeadings} aria-hidden="true">
					<span>Recording</span>
					<span>Date</span>
					<span>Video</span>
					<span>Size</span>
				</div>
			)}
			<ul className={view === "grid" ? styles.grid : styles.rows}>
				{items.map((item) => {
					const title = item.title || item.name;
					const date = item.medalCreatedMs ?? item.modifiedMs;
					const dateMeaning =
						item.medalCreatedMs != null ? "Medal created" : "File modified";
					const unavailable = item.availability !== "available";
					const problem = unavailable
						? item.availability === "missing"
							? "File missing"
							: "File inaccessible"
						: item.status === "error"
							? "Inspection failed"
							: "";
					return (
						<li key={item.id}>
							<button
								className={styles.recording}
								data-recording-id={item.id}
								disabled={selecting || unavailable}
								aria-label={"Play " + title}
								aria-current={selectedId === item.id ? "true" : undefined}
								title={
									item.name +
									"\n" +
									item.path +
									(problem ? "\n" + (item.error || problem) : "")
								}
								onClick={() => onOpen(item.id)}
							>
								<span className={styles.preview}>
									<Thumbnail recording={item} active={active} root={root} />
									{item.duration !== null && (
										<span className={styles.duration}>
											{time(item.duration)}
										</span>
									)}
								</span>
								<span className={styles.identity}>
									<strong>{title}</strong>
									<span className={styles.cardDetails}>
										{problem ? (
											<span className={styles.problem}>{problem}</span>
										) : (
											<span title={dateMeaning + ": " + modifiedDate(date)}>
												{item.game || modifiedDate(date)}
											</span>
										)}
										<span>{videoLabel(item)}</span>
									</span>
								</span>
								<span className={styles.modified}>
									<span title={dateMeaning}>{modifiedDate(date)}</span>
								</span>
								<span className={styles.media}>{videoLabel(item)}</span>
								<span className={styles.file}>{bytes(item.size)}</span>
							</button>
						</li>
					);
				})}
			</ul>
		</div>
	);
}
