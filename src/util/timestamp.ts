function getOrdinal(day: number): string {
  const mod10 = day % 10;
  const mod100 = day % 100;

  if (mod10 === 1 && mod100 !== 11) return `${day}st`;
  if (mod10 === 2 && mod100 !== 12) return `${day}nd`;
  if (mod10 === 3 && mod100 !== 13) return `${day}rd`;
  return `${day}th`;
}

export function formatTimestamp(timestamp: number | string | Date): string {
  const date = new Date(timestamp);

  const weekday = new Intl.DateTimeFormat('en-GB', {
    weekday: 'long'
  }).format(date);

  const day = getOrdinal(date.getDate());

  const month = new Intl.DateTimeFormat('en-GB', {
    month: 'long'
  }).format(date);

  const year = date.getFullYear();

  let time = new Intl.DateTimeFormat('en-GB', {
    hour: 'numeric',
    minute: '2-digit',
    second: '2-digit',
    hour12: true
  }).format(date);

  // Convert "pm" -> "p.m." and "am" -> "a.m."
  time = time.replace(/\s?pm/i, ' p.m.').replace(/\s?am/i, ' a.m.');

  return `${weekday}, ${day} ${month} ${year} at ${time}`;
}
