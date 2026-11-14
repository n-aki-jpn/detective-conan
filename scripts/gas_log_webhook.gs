/**
 * 名探偵コナンTCG 対戦サーバー用 Google スプレッドシート ログ追記 Webhook (GAS)
 *
 * 対象スプレッドシートID: 1M1Oa95wZXykEPeYHDg_kynX9VbSV5yOdMSMWwjY7O8w
 *
 * 【デプロイ手順】
 * 1. スプレッドシートを開き、メニュー「拡張機能」>「Apps Script」を開く。
 * 2. このファイルの内容を貼り付けて保存する。
 * 3. 右上「デプロイ」>「新しいデプロイ」> 種類「ウェブアプリ」を選択。
 * 4. 「次のユーザーとして実行」: 自分、「アクセスできるユーザー」: 全員 に設定してデプロイ。
 * 5. 発行されたウェブアプリ URL を環境変数 `GAS_LOG_WEBHOOK_URL` に設定して conan_server を起動する。
 *    例 (PowerShell):
 *      $env:GAS_LOG_WEBHOOK_URL = "https://script.google.com/macros/s/xxxxx/exec"
 *      .\conan_server\target\release\conan_server.exe 8000
 */

const SPREADSHEET_ID = "1M1Oa95wZXykEPeYHDg_kynX9VbSV5yOdMSMWwjY7O8w";
const SHEET_NAME = "シート1"; // 必要に応じてシート名を変更してください

function doPost(e) {
  try {
    const body = JSON.parse(e.postData.contents);
    const ss = SpreadsheetApp.openById(SPREADSHEET_ID);
    const sheet = ss.getSheetByName(SHEET_NAME) || ss.getSheets()[0];

    // ヘッダーがなければ自動作成
    if (sheet.getLastRow() === 0) {
      sheet.appendRow(["日時", "Room ID", "種別", "内容"]);
    }

    // rows 配列で複数行まとめて受け取る形式と、単一行形式の両方に対応
    const rows = Array.isArray(body.rows) ? body.rows : [body];
    const values = rows.map((r) => [
      r.timestamp ? new Date(r.timestamp * 1000) : new Date(),
      r.roomId || "",
      r.type || "LOG",
      r.message || "",
    ]);

    if (values.length > 0) {
      const startRow = sheet.getLastRow() + 1;
      sheet.getRange(startRow, 1, values.length, 4).setValues(values);
    }

    return ContentService.createTextOutput(
      JSON.stringify({ ok: true, count: values.length })
    ).setMimeType(ContentService.MimeType.JSON);
  } catch (err) {
    return ContentService.createTextOutput(
      JSON.stringify({ ok: false, error: String(err) })
    ).setMimeType(ContentService.MimeType.JSON);
  }
}
