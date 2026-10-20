/**
 * Active smart-view selection state (RD-M9-014~021).
 *
 * Kept in a dependency-free leaf module so both `task-store` (which renders
 * the projection) and `smart-view-store` (which evaluates it) can share the
 * refs without an import cycle.
 *
 * A smart view is a read-only projection: `keys` is the set of task keys the
 * active view selects (null = no smart view active). It never carries
 * editable business data.
 */
import { ref } from "vue";
import type { SmartViewKind } from "../ipc/types";

export const activeSmartView = ref<SmartViewKind | null>(null);

/** Keys selected by the active flat smart view (null = view inactive). */
export const smartViewKeys = ref<ReadonlySet<string> | null>(null);

/** Keys selected by one group of a grouping view (by_tag drill-down). */
export const smartViewGroupKeys = ref<ReadonlySet<string> | null>(null);

export function clearSmartViewSelection(): void {
  activeSmartView.value = null;
  smartViewKeys.value = null;
  smartViewGroupKeys.value = null;
}
