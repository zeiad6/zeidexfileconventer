import { createRoot } from 'react-dom/client';
import { I18nProvider } from '../lib/i18n';
import { Jobs } from '../jobs/Jobs';
import '../styles.css';

createRoot(document.getElementById('root')!).render(
  <I18nProvider>
    <Jobs />
  </I18nProvider>,
);
