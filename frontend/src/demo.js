// 仅用于界面设计预览，不来自网络、浏览器或当前电脑。
export const demoDate = "2026-10-25";
export const devices = [
  {
    id: "studio-linux",
    name: "Studio Linux",
    platform: "KDE / Wayland",
    agent: "0.1.0",
    observed: "17:06:24",
    timezone: "Asia/Shanghai",
  },
  {
    id: "travel-windows",
    name: "Travel Windows",
    platform: "Windows 11",
    agent: "0.1.0",
    observed: "15:42:10",
    timezone: "Asia/Shanghai",
  },
];
const studio = [
  {
    id: "com.visualstudio.code",
    name: "Visual Studio Code",
    short: "VS",
    process: "code",
    tone: "lime",
    seconds: 7200,
    week: 26900,
    sessions: 8,
    last: "17:06",
    current: true,
    session: 872,
  },
  {
    id: "org.mozilla.firefox",
    name: "Firefox",
    short: "Fx",
    process: "firefox",
    tone: "orange",
    seconds: 4080,
    week: 21000,
    sessions: 19,
    last: "16:48",
  },
  {
    id: "com.figma.desktop",
    name: "Figma",
    short: "Fi",
    process: "figma",
    tone: "lavender",
    seconds: 3000,
    week: 12100,
    sessions: 5,
    last: "15:36",
  },
  {
    id: "org.kde.konsole",
    name: "Konsole",
    short: ">_",
    process: "konsole",
    tone: "dark",
    seconds: 1800,
    week: 9800,
    sessions: 12,
    last: "16:51",
  },
  {
    id: "com.slack.desktop",
    name: "Slack",
    short: "Sl",
    process: "slack",
    tone: "pink",
    seconds: 600,
    week: 5400,
    sessions: 6,
    last: "14:12",
  },
  {
    id: "md.obsidian",
    name: "Obsidian",
    short: "Ob",
    process: "obsidian",
    tone: "blue",
    seconds: 300,
    week: 3120,
    sessions: 3,
    last: "13:05",
  },
];
const travel = [
  {
    id: "com.visualstudio.code",
    name: "Visual Studio Code",
    short: "VS",
    process: "Code.exe",
    tone: "lime",
    seconds: 4800,
    week: 18400,
    sessions: 6,
    last: "15:42",
    current: true,
    session: 324,
  },
  {
    id: "org.mozilla.firefox",
    name: "Firefox",
    short: "Fx",
    process: "firefox.exe",
    tone: "orange",
    seconds: 2160,
    week: 9100,
    sessions: 12,
    last: "15:35",
  },
  {
    id: "com.figma.desktop",
    name: "Figma",
    short: "Fi",
    process: "Figma.exe",
    tone: "lavender",
    seconds: 1200,
    week: 4600,
    sessions: 2,
    last: "14:10",
  },
];
export function sampleApps(device) {
  return device === "travel-windows" ? travel : studio;
}
export function formatDuration(seconds = 0, detailed = false) {
  const hours = Math.floor(seconds / 3600),
    minutes = Math.floor((seconds % 3600) / 60),
    rest = seconds % 60;
  if (detailed)
    return [hours, minutes, rest]
      .map((n) => String(n).padStart(2, "0"))
      .join(":");
  return hours
    ? `${hours} 小时 ${minutes} 分`
    : `${minutes} 分${rest ? ` ${rest} 秒` : ""}`;
}
