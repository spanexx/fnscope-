import React from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';

// main.tsx is the mount point only; App owns data loading and shell state.
const container = document.getElementById('root');
if (!container) {
  throw new Error('fnscope: #root container not found');
}

createRoot(container).render(<App />);
