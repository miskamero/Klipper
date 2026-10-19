import { useRef } from "react";
import { unfinished, type ExportJob } from "../../services/exports";
import styles from "./ExportQueue.module.scss";

function jobStatus(job: ExportJob, position: number) {
	if (job.phase === "queued") return `Queued · position ${position}`;
	if (job.phase === "encoding")
		return `Encoding · ${Math.min(100, Math.floor((100 * job.seconds) / job.duration))}%`;
	return {
		preparing: "Preparing…",
		verifying: "Verifying video/audio…",
		completed: "Completed",
		cancelled: "Cancelled",
		error: "Failed",
	}[job.phase];
}

export default function ExportQueue({
	jobs,
	expanded,
	onToggle,
	onAction,
	onOpen,
	error,
}: {
	jobs: ExportJob[];
	expanded: boolean;
	onToggle: () => void;
	onAction: (command: "exports_cancel" | "exports_dismiss", id: number) => void;
	onOpen: (id: number) => void;
	error: string;
}) {
	const toggle = useRef<HTMLButtonElement>(null);
	const running = jobs.filter((job) => unfinished(job) && job.phase !== "queued").length;
	const queued = jobs.filter((job) => job.phase === "queued");
	return (
		<aside className={styles.queue} aria-label="Export queue">
			<button
				ref={toggle}
				className={styles.summary}
				aria-expanded={expanded}
				aria-controls="export-jobs"
				onClick={onToggle}
			>
				<strong>Exports</strong>
				<span>
					{running} running · {queued.length} queued
					{!running && !queued.length ? ` · ${jobs.length} finished` : ""}
				</span>
				<span aria-hidden="true">{expanded ? "⌄" : "⌃"}</span>
			</button>
			{expanded && (
				<div
					id="export-jobs"
					className={styles.jobs}
					onKeyDown={(event) => {
						if (event.key === "Escape") {
							onToggle();
							toggle.current?.focus();
						}
					}}
				>
					<p className={styles.hint}>
						Exports run while Klipper is open. Closing cancels unfinished jobs.
					</p>
					{jobs.map((job) => (
						<article key={job.id} className={styles.job}>
							<strong title={job.path}>
								{job.path.split(/[\\/]/).pop() || job.name}
							</strong>
							<div className={styles.status}>
								<span>
									{jobStatus(
										job,
										queued.findIndex((item) => item.id === job.id) + 1,
									)}
								</span>
								{job.eta != null && job.phase === "encoding" && (
									<span>About {Math.ceil(job.eta)}s left</span>
								)}
							</div>
							{job.phase === "encoding" && (
								<progress
									aria-label={`Export progress for ${job.name}`}
									max={job.duration}
									value={job.seconds}
								/>
							)}
							{job.notice && <p>{job.notice}</p>}
							{job.error && <p role="alert">{job.error}</p>}
							<div className={styles.actions}>
								{job.phase !== "queued" && (
									<span className={styles.hint}>
										{job.encoder === "nvenc" ? "NVIDIA NVENC" : "CPU H.264"}
									</span>
								)}
								{job.phase === "completed" && (
									<button onClick={() => onOpen(job.id)}>Open recording</button>
								)}
								<button
									onClick={() =>
										onAction(
											unfinished(job) ? "exports_cancel" : "exports_dismiss",
											job.id,
										)
									}
								>
									{unfinished(job) ? "Cancel export" : "Dismiss"}
								</button>
							</div>
						</article>
					))}
				</div>
			)}
			{error && <p role="alert">{error}</p>}
		</aside>
	);
}
