/** About and Donate dialogs. Both are plain dialogs with their own backdrop so they behave the
 *  same in a transparent window as in a browser preview. */

import { useState, type ReactNode } from 'react';
import { copyText, openExternal } from '../lib/api';
import { useI18n } from '../lib/i18n';

const DEV_URL = 'https://zeidexlink.zeidexsoft.com/zeidex';
const SITE_URL = 'https://zeidexsoft.com';
const KURAIMI = '3060799480';
const USDT = 'TPLD2d7H8eXjYVqK7mNbZp8eW9aC24Zeix';
const TON = 'EQB_zeidexsoft_donation_ton_wallet_v4r2';

function Shell({ id, wide, onClose, children }: { id: string; wide?: boolean; onClose: () => void; children: ReactNode }) {
  return (
    <div
      className="fixed inset-0 z-40 grid place-items-center bg-black/60 p-4 backdrop-blur-sm"
      onPointerDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <dialog
        id={id}
        open
        role="dialog"
        aria-modal="true"
        className={`modal${wide ? ' modal-wide' : ''}`}
        style={{ position: 'relative', inset: 'auto', margin: 0 }}
      >
        <div className="modal-card">{children}</div>
      </dialog>
    </div>
  );
}

function CopyBox({ id, label, value }: { id?: string; label?: string; value: string }) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  return (
    <div className="flex flex-col gap-1">
      {label && <span className="text-[11px] font-semibold text-muted">{label}</span>}
      <div className="copy-box">
        <code id={id}>{value}</code>
        <button
          type="button"
          className={`copy-btn${copied ? ' copied' : ''}`}
          onClick={async () => {
            await copyText(value);
            setCopied(true);
            window.setTimeout(() => setCopied(false), 2000);
          }}
        >
          {copied ? t('copied') : t('copy')}
        </button>
      </div>
    </div>
  );
}

export function AboutModal({ open, onClose, onDonate }: { open: boolean; onClose: () => void; onDonate: () => void }) {
  const { t } = useI18n();
  if (!open) return null;
  return (
    <Shell id="aboutModal" onClose={onClose}>
      <div className="modal-header flex items-start justify-between gap-3 border-b border-line pb-3">
        <div className="flex items-center gap-3">
          <img src="/icon.png" alt="" className="h-[42px] w-[42px] object-contain" />
          <div>
            <h2 className="m-0 text-lg font-bold">{t('aboutTitle')}</h2>
            <div className="badge mt-1 inline-block rounded-xl border border-line bg-bg px-2 py-0.5 text-[11px] font-semibold text-accent">
              {t('version')}
            </div>
          </div>
        </div>
        <button type="button" className="close-btn" id="closeAboutBtn" aria-label={t('close')} onClick={onClose}>
          ✕
        </button>
      </div>

      <div className="modal-body flex flex-col gap-3.5">
        <p className="m-0 text-sm leading-relaxed">{t('aboutDesc')}</p>
        <div className="flex flex-col gap-2.5 rounded-[10px] border border-line bg-bg p-3.5">
          <div className="text-sm font-bold text-accent">{t('developedBy')}</div>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              id="devProfileBtn"
              className="btn-primary-link"
              onClick={() => void openExternal(DEV_URL)}
            >
              <span className="icon">🌐</span> <span>{t('visitDev')}</span>
            </button>
            <button
              type="button"
              id="devWebsiteBtn"
              className="btn-secondary-link"
              onClick={() => void openExternal(SITE_URL)}
            >
              <span className="icon">🔗</span> <span>{t('visitSite')}</span>
            </button>
          </div>
        </div>
      </div>

      <div className="modal-footer flex justify-end gap-2 border-t border-line pt-3.5">
        <button type="button" id="aboutDonateBtn" className="btn-action" onClick={onDonate}>
          <span className="icon">❤️</span> <span>{t('donate')}</span>
        </button>
        <button type="button" id="closeAboutBtn2" className="btn-secondary" onClick={onClose}>
          {t('close')}
        </button>
      </div>
    </Shell>
  );
}

export function DonateModal({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = useI18n();
  if (!open) return null;
  return (
    <Shell id="donateModal" wide onClose={onClose}>
      <div className="modal-header flex items-start justify-between gap-3 border-b border-line pb-3">
        <div>
          <h2 className="m-0 text-lg font-bold">❤️ {t('donateTitle')}</h2>
          <div className="mt-1 text-[13px] text-muted">{t('paymentMethods')}</div>
        </div>
        <button type="button" className="close-btn" id="closeDonateBtn" aria-label={t('close')} onClick={onClose}>
          ✕
        </button>
      </div>

      <div className="modal-body flex flex-col gap-3.5">
        <p className="m-0 text-sm leading-relaxed">{t('donateDesc')}</p>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <div className="pay-card">
            <div className="flex items-center gap-2.5">
              <span className="text-2xl">🏦</span>
              <div>
                <h3 className="m-0 text-sm font-bold">{t('kuraimi')}</h3>
                <span className="text-[11px] text-muted">Yemen / Regional</span>
              </div>
            </div>
            <p className="m-0 text-xs leading-snug text-muted">{t('kuraimiDesc')}</p>
            <CopyBox value={KURAIMI} />
          </div>

          <div className="pay-card">
            <div className="flex items-center gap-2.5">
              <span className="text-2xl">⭐</span>
              <div>
                <h3 className="m-0 text-sm font-bold">{t('stars')}</h3>
                <span className="text-[11px] text-muted">Instant Stars</span>
              </div>
            </div>
            <p className="m-0 text-xs leading-snug text-muted">{t('starsDesc')}</p>
            <button
              type="button"
              id="starsLinkBtn"
              className="btn-secondary-link"
              onClick={() => void openExternal(DEV_URL)}
            >
              <span className="icon">✈️</span> <span>{t('onlineDonation')}</span>
            </button>
          </div>

          <div className="pay-card sm:col-span-2">
            <div className="flex items-center gap-2.5">
              <span className="text-2xl">🪙</span>
              <div>
                <h3 className="m-0 text-sm font-bold">{t('crypto')}</h3>
                <span className="text-[11px] text-muted">Crypto / Blockchain</span>
              </div>
            </div>
            <p className="m-0 text-xs leading-snug text-muted">{t('cryptoDesc')}</p>
            <div className="flex flex-col gap-2">
              <CopyBox label="USDT (TRC-20):" value={USDT} />
              <CopyBox label="TON Wallet:" value={TON} />
            </div>
          </div>

          <div className="pay-card sm:col-span-2">
            <div className="flex items-center gap-2.5">
              <span className="text-2xl">💳</span>
              <div>
                <h3 className="m-0 text-sm font-bold">{t('paypal')}</h3>
                <span className="text-[11px] text-muted">Visa / Mastercard / PayPal</span>
              </div>
            </div>
            <p className="m-0 text-xs leading-snug text-muted">{t('paypalDesc')}</p>
            <div className="flex flex-wrap gap-2">
              <button
                type="button"
                id="onlinePayBtn"
                className="btn-primary-link"
                onClick={() => void openExternal(DEV_URL)}
              >
                <span className="icon">🌐</span> <span>{t('onlineDonation')}</span>
              </button>
              <button
                type="button"
                id="websitePayBtn"
                className="btn-secondary-link"
                onClick={() => void openExternal(SITE_URL)}
              >
                <span className="icon">🔗</span> <span>{t('visitSite')}</span>
              </button>
            </div>
          </div>
        </div>
      </div>

      <div className="modal-footer flex justify-end gap-2 border-t border-line pt-3.5">
        <button type="button" id="closeDonateBtn2" className="btn-secondary" onClick={onClose}>
          {t('close')}
        </button>
      </div>
    </Shell>
  );
}
