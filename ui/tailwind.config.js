/** @type {import('tailwindcss').Config} */
module.exports = {
  // The explorer is always dark; `class` keeps Tailwind's dark variants
  // available without forcing a media query on embedded consumers.
  darkMode: 'class',
  content: ['./src/**/*.{ts,tsx}'],
  theme: {
    extend: {},
  },
  plugins: [],
};
