// Runs before the page first paints: the loading screen's logo and font,
// cached from an earlier visit's game files, so they show at once.
try {
  const logo = localStorage.getItem('pomme-logo');
  if (logo) document.documentElement.style.setProperty('--logo', `url(${logo})`);
} catch {}
