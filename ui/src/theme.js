// Apply the persisted palette before first paint; Paper remains the default.
// Served as its own file so the strict CSP needs no 'unsafe-inline' exception.
try {
  var saved = window.localStorage.getItem('fnscope-workspace-theme');
  if (saved === 'paper' || saved === 'graphite') {
    document.documentElement.setAttribute('data-theme', saved);
  }
} catch (error) {
  // Blocked storage keeps the server-rendered Paper default.
}
