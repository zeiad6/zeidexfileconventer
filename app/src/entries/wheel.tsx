import { createRoot } from 'react-dom/client';
import { I18nProvider } from '../lib/i18n';
import '../styles.css';
import { Wheel } from '../wheel/Wheel';

createRoot(document.getElementById('root')!).render(
  <I18nProvider>
    <Wheel />
  </I18nProvider>,
);
