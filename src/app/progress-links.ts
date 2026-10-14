/**
 * Progress link helpers (RD-M6-023~030).
 *
 * Provider detection mirrors the backend rule set so the icon shown in the
 * inspector matches what gets persisted. URL validation is client-side
 * pre-flight only — the backend remains authoritative and must reject
 * non-http(s) schemes on its own.
 */
import type { ProgressLinkProvider } from "../ipc/types";

interface ProviderRule {
  provider: ProgressLinkProvider;
  label: string;
  icon: string;
  test: RegExp;
}

const RULES: ProviderRule[] = [
  { provider: "github", label: "GitHub", icon: "fab fa-github", test: /(^|\.)github\.(com|io)$/i },
  { provider: "gitlab", label: "GitLab", icon: "fab fa-gitlab", test: /(^|\.)gitlab\.(com|io)$/i },
  { provider: "linear", label: "Linear", icon: "fas fa-chart-line", test: /(^|\.)linear\.app$/i },
  { provider: "jira", label: "Jira", icon: "fab fa-jira", test: /(^|\.)atlassian\.(net|com)$/i },
  {
    provider: "azure",
    label: "Azure DevOps",
    icon: "fab fa-microsoft",
    test: /(^|\.)visualstudio\.com$|(^|\.)dev\.azure\.com$/i,
  },
];

const FALLBACK = {
  provider: "generic" as ProgressLinkProvider,
  label: "Link",
  icon: "fas fa-link",
};

export function detectProvider(url: string): ProgressLinkProvider {
  const host = hostOf(url);
  if (!host) return "generic";
  for (const rule of RULES) {
    if (rule.test.test(host)) return rule.provider;
  }
  return "generic";
}

export function providerLabel(provider: string | undefined, url = ""): string {
  const rule = RULES.find((r) => r.provider === provider);
  if (rule) return rule.label;
  if (!provider || provider === "generic") {
    const detected = RULES.find((r) => r.provider === detectProvider(url));
    return detected?.label ?? FALLBACK.label;
  }
  // Unknown provider string: title-case it rather than dropping information.
  return provider.charAt(0).toUpperCase() + provider.slice(1);
}

export function providerIcon(provider: string | undefined, url = ""): string {
  const rule = RULES.find((r) => r.provider === provider);
  if (rule) return rule.icon;
  if (!provider || provider === "generic") {
    const detected = RULES.find((r) => r.provider === detectProvider(url));
    return detected?.icon ?? FALLBACK.icon;
  }
  return FALLBACK.icon;
}

export function hostOf(url: string): string | null {
  try {
    return new URL(url).hostname;
  } catch {
    return null;
  }
}

export interface UrlValidation {
  valid: boolean;
  reason: string | null;
}

/** Only http/https are accepted; javascript:/data:/file: are rejected. */
export function validateProgressUrl(url: string): UrlValidation {
  const value = url.trim();
  if (!value) return { valid: false, reason: "URL is required" };
  let parsed: URL;
  try {
    parsed = new URL(value);
  } catch {
    return { valid: false, reason: "Not a valid absolute URL" };
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    return { valid: false, reason: `Unsupported scheme "${parsed.protocol}" — only http(s) is allowed` };
  }
  if (!parsed.hostname) return { valid: false, reason: "URL has no host" };
  return { valid: true, reason: null };
}
