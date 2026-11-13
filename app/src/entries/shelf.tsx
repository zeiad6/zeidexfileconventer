import { createRoot } from 'react-dom/client';
import { I18nProvider } from '../lib/i18n';
import { Shelf } from '../shelf/Shelf';
import '../styles.css';

createRoot(document.getElementById('root')!).render(
  <I18nProvider>
    <Shelf />
  </I18nProvider>,
);
