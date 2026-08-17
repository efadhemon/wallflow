function platformHint(): string {
  const uaData = (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData;
  return `${uaData?.platform ?? ""} ${navigator.platform} ${navigator.userAgent}`;
}

/** True on a Linux desktop webview. Android also reports Linux in the UA. */
export function isLinuxDesktop(): boolean {
  const hint = platformHint();
  return /linux/i.test(hint) && !/android/i.test(hint);
}

/** True on a Windows desktop webview. */
export function isWindowsDesktop(): boolean {
  return /win/i.test(platformHint());
}

/** True on a macOS desktop webview. iPhone/iPad also report Mac in some UAs. */
export function isMacDesktop(): boolean {
  const hint = platformHint();
  return /mac/i.test(hint) && !/iphone|ipad|ipod|ios/i.test(hint);
}

/** Linux and Windows draw an in-page titlebar; macOS keeps native chrome. */
export function usesCustomTitlebar(): boolean {
  return isLinuxDesktop() || isWindowsDesktop();
}
