// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import ExportQueue from "./ExportQueue";
import type { ExportJob } from "../../services/exports";

const job: ExportJob = {
	id: 1,
	name: "Source.mp4",
	path: "C:/synthetic/First.mp4",
	phase: "encoding",
	seconds: 3,
	duration: 12,
	eta: 9,
	encoder: "cpu",
	notice: "",
	error: "",
};
afterEach(cleanup);
it("shows actual job progress, FIFO positions, cancellation and completed outputs", () => {
	const action = vi.fn(),
		open = vi.fn();
	render(
		<ExportQueue
			jobs={[
				job,
				{ ...job, id: 2, phase: "queued", path: "C:/synthetic/Second.mp4" },
				{ ...job, id: 3, phase: "completed", path: "C:/synthetic/Done.mp4" },
				{
					...job,
					id: 4,
					phase: "error",
					error: "Disk full",
					path: "C:/synthetic/Failed.mp4",
				},
			]}
			expanded
			onToggle={vi.fn()}
			onAction={action}
			onOpen={open}
			error=""
		/>,
	);
	expect(screen.getByText("1 running · 1 queued")).toBeTruthy();
	expect(screen.getByText("Encoding · 25%")).toBeTruthy();
	expect(screen.getByText("Queued · position 1")).toBeTruthy();
	expect((screen.getByRole("progressbar") as HTMLProgressElement).value).toBe(3);
	fireEvent.click(screen.getAllByText("Cancel export")[1]);
	expect(action).toHaveBeenCalledWith("exports_cancel", 2);
	fireEvent.click(screen.getByText("Open recording"));
	expect(open).toHaveBeenCalledWith(3);
	fireEvent.click(screen.getAllByText("Dismiss")[1]);
	expect(action).toHaveBeenCalledWith("exports_dismiss", 4);
	expect(screen.getByRole("alert").textContent).toBe("Disk full");
});
it("collapsing hides details without issuing cancellation; Escape restores focus", () => {
	const action = vi.fn(),
		toggle = vi.fn();
	const { rerender } = render(
		<ExportQueue
			jobs={[job]}
			expanded
			onToggle={toggle}
			onAction={action}
			onOpen={vi.fn()}
			error=""
		/>,
	);
	fireEvent.keyDown(screen.getByText("Cancel export"), { key: "Escape" });
	expect(toggle).toHaveBeenCalled();
	expect(document.activeElement).toBe(screen.getByRole("button", { expanded: true }));
	rerender(
		<ExportQueue
			jobs={[job]}
			expanded={false}
			onToggle={toggle}
			onAction={action}
			onOpen={vi.fn()}
			error=""
		/>,
	);
	expect(screen.queryByText("Cancel export")).toBeNull();
	expect(action).not.toHaveBeenCalled();
});
