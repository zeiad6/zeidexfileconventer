import { createRoot } from 'react-dom/client';
import { Home } from '../home/Home';
import { I18nProvider } from '../lib/i18n';
import '../styles.css';

createRoot(document.getElementById('root')!).render(
  <I18nProvider>
    <Home />
  </I18nProvider>,
);
