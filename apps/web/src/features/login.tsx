import { Landmark, LockKeyhole, Users } from "lucide-react";
import { Logo } from "./shell";

export function Login({
  configured,
  connected,
  loginUrl,
}: {
  configured: boolean;
  connected: boolean;
  loginUrl: string;
}) {
  return (
    <main className="login">
      <section className="login-intro">
        <div className="brand">
          <Logo size={34} />
          <span>Money Track</span>
        </div>
        <h1>
          你的錢，
          <br />
          一眼看清楚。
        </h1>
        <p className="lead">記帳、資產、預算與投資，放在一個只屬於你的地方。</p>
        <ul className="values">
          <li>
            <Landmark size={20} />
            <span>
              <strong>全部帳戶一次看</strong>
              <small>現金、銀行、信用卡、股票，自動算出淨資產</small>
            </span>
          </li>
          <li>
            <LockKeyhole size={20} />
            <span>
              <strong>資料留在自己的主機</strong>
              <small>不上傳第三方雲端，銀行密碼加密保存</small>
            </span>
          </li>
          <li>
            <Users size={20} />
            <span>
              <strong>和家人唯讀分享</strong>
              <small>選擇要分享的帳戶，隨時可以撤銷</small>
            </span>
          </li>
        </ul>
      </section>
      <section className="login-card" aria-label="登入">
        <h2>登入</h2>
        <p className="muted">
          使用 Google 帳號登入，第一次登入會自動建立你的空間。
        </p>
        {configured ? (
          <a className="btn btn-google btn-lg block" href={loginUrl}>
            <GoogleIcon />
            使用 Google 帳號繼續
          </a>
        ) : (
          <>
            <button
              type="button"
              className="btn btn-google btn-lg block"
              disabled
            >
              <GoogleIcon />
              Google 登入設定中
            </button>
            <p className="small muted">
              應用程式已就緒，管理員正在完成 Google 登入設定。
            </p>
          </>
        )}
        <p className="small muted">
          只用來驗證身分，Money Track 不會取得你的 Google 密碼、信件或雲端檔案。{" "}
          <a href="/privacy">隱私權說明</a>
        </p>
        <p className={`service ${connected ? "ok" : "wait"}`}>
          <i />
          {connected ? "服務正常" : "正在連線到資料服務…"}
        </p>
      </section>
    </main>
  );
}

function GoogleIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true">
      <path
        fill="#4285F4"
        d="M21.6 12.2c0-.7-.1-1.4-.2-2.1H12v4h5.4a4.7 4.7 0 0 1-2 3v2.6h3.3c1.9-1.8 2.9-4.3 2.9-7.5Z"
      />
      <path
        fill="#34A853"
        d="M12 22c2.7 0 5-.9 6.7-2.4l-3.3-2.6c-.9.6-2 1-3.4 1-2.6 0-4.8-1.8-5.6-4.1H3v2.7A10 10 0 0 0 12 22Z"
      />
      <path
        fill="#FBBC05"
        d="M6.4 13.9a6 6 0 0 1 0-3.8V7.4H3a10 10 0 0 0 0 9.2l3.4-2.7Z"
      />
      <path
        fill="#EA4335"
        d="M12 6c1.5 0 2.8.5 3.9 1.5L18.8 4A9.7 9.7 0 0 0 12 2a10 10 0 0 0-9 5.4l3.4 2.7C7.2 7.8 9.4 6 12 6Z"
      />
    </svg>
  );
}
