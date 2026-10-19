export function localizedDate(value: unknown): string | null {
	if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) return null;
	const date = new Date(value);
	return Number.isNaN(date.getTime())
		? null
		: date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

export function present(value: unknown): boolean {
	if (value == null || value === "") return false;
	if (typeof value === "string") return !!value.trim();
	if (Array.isArray(value)) return value.some(present);
	if (typeof value === "object") return Object.values(value).some(present);
	return true;
}

export function commonClaim(values: unknown[]) {
	const available = values.filter(present);
	const distinct = new Set(available.map((value) => JSON.stringify(value)));
	return { value: distinct.size === 1 ? available[0] : undefined, differs: distinct.size > 1 };
}
