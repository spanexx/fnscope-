// PostCSS pipeline used by the Tailwind CLI: expand utilities, then add
// vendor prefixes so the generated sheet works across browser engines.
module.exports = {
  plugins: {
    tailwindcss: {},
    autoprefixer: {},
  },
};
