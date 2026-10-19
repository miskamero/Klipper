import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Recording } from "./media";

export type Encoder = "auto" | "cpu" | "nvenc";
export interface ExportJob {
	id: number;
	name: string;
	path: string;
	phase: "queued" | "preparing" | "encoding" | "verifying" | "completed" | "cancelled" | "error";
	seconds: number;
	duration: number;
	eta: number | null;
	encoder: Encoder;
	notice: string;
	error: string;
}
interface ExportSnapshot {
	revision: number;
	jobs: ExportJob[];
}
export const unfinished = (job: ExportJob) =>
	["queued", "preparing", "encoding", "verifying"].includes(job.phase);
export const openExport = (id: number) => invoke<Recording>("exports_open", { id });

// App owns this subscription. Hiding the floating view never affects job execution/events.
export function useExports() {
	const [jobs, setJobs] = useState<ExportJob[]>([]);
	const [error, setError] = useState("");
	const revision = useRef(0);
	const alive = useRef(true);
	const refresh = useCallback(async () => {
		if (!isTauri()) return;
		try {
			const snapshot = await invoke<ExportSnapshot>("exports_status");
			if (alive.current && snapshot.revision > revision.current) {
				revision.current = snapshot.revision;
				setJobs(snapshot.jobs);
			}
		} catch (reason) {
			if (alive.current) setError(String(reason));
		}
	}, []);
	useEffect(() => {
		if (!isTauri()) return;
		alive.current = true;
		let disposed = false;
		const subscription = listen<ExportSnapshot>("exports-changed", ({ payload }) => {
			if (!disposed && payload.revision > revision.current) {
				revision.current = payload.revision;
				setJobs(payload.jobs);
			}
		});
		void subscription
			.then(async (stop) => {
				if (disposed) return stop();
				const snapshot = await invoke<ExportSnapshot>("exports_status");
				if (!disposed && snapshot.revision > revision.current) {
					revision.current = snapshot.revision;
					setJobs(snapshot.jobs);
				}
			})
			.catch((reason) => {
				if (!disposed) setError(String(reason));
			});
		return () => {
			alive.current = false;
			disposed = true;
			void subscription.then((stop) => stop()).catch(() => {});
		};
	}, []);
	async function action(command: "exports_cancel" | "exports_dismiss", id: number) {
		setError("");
		try {
			await invoke(command, { id });
			await refresh();
		} catch (reason) {
			setError(String(reason));
		}
	}
	return { jobs, error, refresh, action };
}
