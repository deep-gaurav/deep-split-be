use serde::{Deserialize, Serialize};

use crate::REQWEST_CLIENT;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailPayload {
    pub from: EmailContact,
    pub reply_to: Vec<EmailContact>,
    pub to: Vec<EmailContact>,
    pub cc: Vec<EmailContact>,
    pub bcc: Vec<EmailContact>,
    pub subject: String,
    pub content: Vec<EmailContent>,
}

#[derive(Serialize, Deserialize)]
pub struct EmailContact {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub email: String,
}

#[derive(Serialize, Deserialize)]
pub struct EmailContent {
    pub mime: String,
    pub value: String,
}

pub async fn send_email_otp(to_email: &str, otp: &str) -> anyhow::Result<()> {
    let auth_tok = std::env::var("EMAIL_AUTH_TOK")?;
    let email_payload = EmailPayload {
        from: EmailContact {
            name: "Split Buddy".to_string().into(),
            email: "split@deepwith.in".to_string(),
        },
        reply_to: vec![],
        cc: vec![],
        bcc: vec![],
        to: vec![EmailContact {
            name: None,
            email: to_email.to_string(),
        }],
        subject: "Your One-Time Passcode for SplitBuddy Signup/Login".to_string(),
        content: vec![
            EmailContent{
                mime:"text/html".to_string(),
                value: r##"
<div style="font-family: Helvetica,Arial,sans-serif;min-width:1000px;overflow:auto;line-height:2">
  <div style="margin:50px auto;width:70%;padding:20px 0">
    <div style="border-bottom:1px solid #eee">
      <a href="" style="font-size:1.4em;color: #00466a;text-decoration:none;font-weight:600">Split Buddy</a>
    </div>
    <p style="font-size:1.1em">Hi,</p>
    <p>This is your One-Time Passcode for SplitBuddy Signup/Login. Passcode is valid for 5 minutes</p>
    <h2 style="background: #00466a;margin: 0 auto;width: max-content;padding: 0 10px;color: #fff;border-radius: 4px;">{{{PASSCODE}}}</h2>
    <p style="font-size:0.9em;">Regards,<br />Split Buddy</p>
    <hr style="border:none;border-top:1px solid #eee" />
  </div>
</div>
                "##.replace("{{{PASSCODE}}}", otp)
            }
        ],
    };
    let request = REQWEST_CLIENT
        .post("https://worker-email-production.deepgauravraj.workers.dev/api/email")
        .header("Authorization", auth_tok)
        .json(&email_payload)
        .send()
        .await?;
    if !request.status().is_success() {
        Err(anyhow::anyhow!("Cant send email"))
    } else {
        Ok(())
    }
}
