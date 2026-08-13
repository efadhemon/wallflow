/** True on a Linux desktop webview. Android also reports Linux in the UA. */
export function isLinuxDesktop(): boolean {
  const uaData = (navigator as Navigator & { userAgentData?: { platform?: string } })
    .userAgentData;
  const hint = `${uaData?.platform ?? ""} ${navigator.platform} ${navigator.userAgent}`;
  return /linux/i.test(hint) && !/android/i.test(hint);
}
