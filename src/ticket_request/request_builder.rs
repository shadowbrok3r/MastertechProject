use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use crate::data::{TicketInformation, SystemInformation};

use super::scaffold::{Salesman, Techs};

#[derive(Serialize)]
pub struct Info{
  pub customer_name: String, 
  pub so_num: String, 
  pub hdd_test: String,
  pub ram_test: String,
  pub ssd_test: String,
  pub checkin_notes: String,
  pub recommendations: String,
  pub specs: String,
  pub cps: String,
  pub cust_code: String,
  pub doc_alias: String,
  pub inv_amt: String,
  pub cust_email: String,
  pub last_inv_num: String,
  pub last_inv_amt: String,
  pub total_inv_num: String,
  pub phone1: String,
  pub phone2: String,

  pub final_disk: String,

  pub salesman: String,
  pub checkin_rep: String,
  pub technician: String,
  pub extra_customer_info: String,
}

// #[derive(Serialize)]
pub struct AsanaTask{
    pub task_name: String,
    pub html_notes: String,
    pub assignee: TaskAssignee,
    pub file_attachment: Option<PathBuf>
}

// #[derive(Serialize)]
pub struct TaskAssignee{
    pub salesman: Salesman,
    pub tech: Techs
}

/*
pub fn html_builder(
    ticket_info: TicketInformation, 
    system_info: SystemInformation,
    client: Client,
    scaffold_request: Sender<String>
){
    ;

    if store.as_str() == "RIV"{

    }
    else{
        let mtech_username = dotenv::var("MTECH_EMAIL").unwrap_or("not provided".to_string());
        let mtech_password = dotenv::var("MTECH_PASS").unwrap_or("not provided".to_string());

        let email_content = email_builder();

        let store_email = &ticket_info.jurisdiction.store_email();

        let email = Message::builder()
            .from("TUR SHEET <pcl.mastertech@gmail.com>".parse().unwrap())
            .to("logan.lees@pclaptops.com".parse().unwrap())
            .subject(format!("{cust} - {so_num}"))
            .header(ContentType::TEXT_HTML)
            .body(email_content)
            .unwrap();

        let creds = Credentials::new(mtech_username.to_owned(), mtech_password.to_owned());

        // Open a remote connection to gmail
        let mailer = SmtpTransport::relay("smtp.gmail.com")
            .unwrap()
            .credentials(creds)
            .build();

        output_text += "\n {store_email} {email}";

        // Send the email
        match mailer.send(&email) {
            Ok(_) => println!("Email sent successfully!"),
            Err(e) => {
                output_text += "\n{e:?}";
                println!("Could not send email: {e:?}")
            },
        }
    }
        
}

pub fn asana_html_builder(
    ticket_info: TicketInformation, 
    system_info: SystemInformation,
    send_specs: bool,
    client: Client,
    scaffold_request: Sender<String>
) -> AsanaTask { 
    let mut salesman_map = HashMap::new();
    let mut tech_map = HashMap::new();

    let salesman = &format!("{}", &salesman_cbox);
    let checkin_rep = &ticket_info.user_id;
    let technician = &format!("{}", &techs_cbox);

    salesman_map.insert("Jake", "1202792432658520");
    salesman_map.insert("Danny", "1202791016369879");
    tech_map.insert("Logan", "1199992640930465");
    tech_map.insert("Bread", "1202792432421640");
    tech_map.insert("Taco", "1202792432551073");

    let hdd_test = &format!("{:?}", &hdd_test_cbox);
    let ram_test = &format!("{:?}", &ram_test_cbox);
    let ssd_test = &format!("{:?}", &ssd_test_cbox);

    let checkin_notes = &ticket_info.checkin_notes;
    let recommendations = &recommendations;   
    let task_name = (cust, so_num);
    let assignees = (salesman, technician);

    let mut attached_file: Option<PathBuf> = None;
    
    if let Some(file) = &opened_file{
        attached_file = Some(file.to_path_buf());
    }

    let mut specs = String::new();
    let cps = antivirus_installed.clone();
    let mut final_disk = String::new();
    let mut each_disk = String::new();
                                            
    let cust_code = &ticket_info.cust_code;
    let doc_alias = &ticket_info.doc_alias;
    //let department = &ticket_info.department;
    //let juris = &ticket_info.jurisdiction;
    let inv_amt = &ticket_info.invoice_amnt;
    let cust_email = &ticket_info.customer_email;
    let last_inv_num = &ticket_info.last_invoice_number;
    let last_inv_amt = &ticket_info.last_invoice_amount;
    let total_inv_num = &ticket_info.total_invoice_count;
    let phone1 = &ticket_info.customer_phone_1;
    let phone2 = &ticket_info.customer_phone_2;
    let mut phone_2 = String::new();
    if !phone2.is_empty(){
        phone_2 = format!
        ("
            <tr>
                <td style=\"padding:1px 1px\">Phone #2</td>
                <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{phone2}</td>
            </tr>
        ");
    }

    let extra_customer_info = format!
    ("
        <tr>
            <td style=\"padding:1px 1px\">Customer Code</td>
            <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{cust_code}</td>
        </tr>
        <tr>
            <td style=\"padding:1px 1px\">Phone #</td>
            <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\"><strong>{phone1}</strong></td>
        </tr>
        {phone_2}
        <tr>
            <td style=\"padding:1px 1px\">Customer Email</td>
            <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{cust_email}</td>
        </tr>
        <tr>
            <td style=\"padding:1px 1px\">Current Total</td>
            <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">${inv_amt}</td>
        </tr>
        <tr>
            <td style=\"padding:1px 1px\">Last SI#</td>
            <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{last_inv_num}</td>
        </tr>
        <tr>
            <td style=\"padding:1px 1px\">Last Invoice Total</td>
            <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{last_inv_amt}</td>
        </tr>
        <tr>
            <td style=\"padding:1px 1px\"># of SI's</td>
            <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{total_inv_num}</td>
        </tr>
    ");
    if send_specs == true{
        let system_name = &system_info.system_name;
        let cpu_name = &system_info.cpu_name;
        let total_ram = &system_info.total_ram;
        let gpu = &system_info.gpu.clone().unwrap_or("no gpu detected".to_string());

        for index in 0..disk_num
        {
            if let Some(disk) = disks.get(index)
            {
                let disk_letter = format!("{}", disk.get("letter").and_then(Value::as_str).unwrap_or(""));
                let disk_available = format!("{} Gb", disk.get("available space").and_then(Value::as_str).unwrap_or(""));
                let disk_total = format!("{} Gb", disk.get("total space").and_then(Value::as_str).unwrap_or(""));

                each_disk += &format!
                ("
                    <tr>
                        <td style=\"padding:1px 1px\">        {disk_letter}</td>
                        <td style=\"padding:1px 1px\">        {disk_available}</td>
                        <td style=\"padding:1px 1px\">        {disk_total}</td>
                    </tr>
                ");

                final_disk = format!
                ("
                    <tr>
                        <td style=\"padding:1px 4px\">Letter</td>
                        <td style=\"padding:1px 4px\">Avail Space</td>
                        <td style=\"padding:1px 4px\">Total Space</td>
                    </tr>
                    {each_disk}
                ");

            }
        }

        specs = format!("
            <table>
                <tr>
                    <td style=\"text-align:center;\" colspan=\"3\" data-cell-widths=\"130,200,200\" width=\"450\"
                    >              <code>       Computer Info        </code></td>
                </tr>
                <tr>
                    <td>OS</td>
                    <td colspan=\"2\" data-cell-widths=\"150,150\">{system_name}</td>
                </tr>
                <tr>
                    <td>CPU</td>
                    <td colspan=\"2\" data-cell-widths=\"150,150\">{cpu_name}</td>
                </tr>
                <tr>
                    <td>RAM</td>
                    <td colspan=\"2\" data-cell-widths=\"150,150\">{total_ram} Gb</td>
                </tr>
                <tr>
                    <td>GPU</td>
                    <td colspan=\"2\" data-cell-widths=\"150,150\">{gpu}</td>
                </tr>
                <tr>
                    <td>Antivirus</td>
                    <td colspan=\"2\" data-cell-widths=\"150,150\">{cps}</td>
                </tr>
                <tr>
                    <td>SEB</td>
                    <td colspan=\"2\" data-cell-widths=\"150,150\"></td>
                </tr>
                <tr>
                <td colspan=
                \"3\" data-cell-widths=\"100,200,200\" width=\"400\" style=\"text-align:center;\"
                >                <code>        HDD/SSD info        </code></td>
                </tr>
                {final_disk}
            </table>
        ").trim().to_string();

    }else{
        specs = "Computer information was not sent with ticket".to_string();
    }
    
    let html_notes = format!
    ("
        <body>
            <table>
                <tr>
                    <td style=\"text-align:center;\" colspan=\"3\" data-cell-widths=\"130,130,130\" width=\"390\"
                    >                <code>        {doc_alias} Info        </code>
                    </td>
                </tr>
                <tr>
                    <td style=\"padding:1px 1px\">Salesman</td>
                    <td style=\"padding:1px 1px\">Checkin Rep</td>
                    <td style=\"padding:1px 1px\">Technician</td>
                </tr>
                <tr>
                    <td style=\"padding:1px 4px\">     {salesman}</td>
                    <td style=\"padding:1px 4px\">     {checkin_rep}</td>
                    <td style=\"padding:1px 4px\">     {technician}</td>
                </tr>
                <tr>
                    <td style=\"text-align:center;\" colspan=\"3\" data-cell-widths=\"130,130,130\" width=\"390\"
                    >                <code>           Customer           </code>
                    </td>
                </tr>
                {extra_customer_info}
            </table>
            {specs}
            <ul>
                <li><strong>SSD test:</strong>     {ssd_test}</li>
                <li><strong>HDD test:</strong>     {hdd_test}</li>
                <li><strong>RAM test:</strong>     {ram_test}</li>
            </ul>
            <h2><strong><code>           Notes           </code></strong></h2>
            <ul><li><strong>        Checkin Notes:      </strong>     \n{checkin_notes}</li>
                <li><strong>        Recommendations:        </strong>     \n{recommendations}</li></ul>
        </body>"
    );

    AsanaTask { 
        task_name: format!("{cust} - {so_num}"), 
        html_notes: html_notes, 
        assignee: TaskAssignee { 
            salesman: (), 
            tech: () 
        }, 
        file_attachment: () 
    }

}


*/
 pub fn email_builder(info: Info) -> String {

  let customer_name = info.customer_name;
  let so_num = info.so_num;
  let hdd_test = info.hdd_test;
  let ram_test = info.ram_test;
  let ssd_test = info.ssd_test;
  let checkin_notes = info.checkin_notes;
  let recommendations = info.recommendations;
  let specs = info.specs;
  let cps = info.cps;
  let cust_code = info.cust_code;
  let doc_alias = info.doc_alias;
  let inv_amt = info.inv_amt;
  let cust_email = info.cust_email;
  let last_inv_num = info.last_inv_num;
  let last_inv_amt = info.last_inv_amt;
  let total_inv_num = info.total_inv_num;
  let phone1 = info.phone1;
  let phone2 = info.phone2;
  let final_disk = info.final_disk;
  let salesman = info.salesman;
  let checkin_rep = info.checkin_rep;
  let technician = info.technician;
  let extra_customer_info = info.extra_customer_info;


  let html_string = format!("
  <!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\" \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd\">
  <html dir=\"ltr\" xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:o=\"urn:schemas-microsoft-com:office:office\" lang=\"en\">
   <head>
    <meta charset=\"UTF-8\">
    <meta content=\"width=device-width, initial-scale=1\" name=\"viewport\">
    <meta name=\"x-apple-disable-message-reformatting\">
    <meta http-equiv=\"X-UA-Compatible\" content=\"IE=edge\">
    <meta content=\"telephone=no\" name=\"format-detection\">
    <title>Mtech template</title>
    <link href=\"https://fonts.googleapis.com/css2?family=Montserrat:wght@400;700&display=swap\" rel=\"stylesheet\">
    <link href=\"https://fonts.googleapis.com/css?family=Open+Sans:400,400i,700,700i\" rel=\"stylesheet\">
    <style type=\"text/css\">
  #outlook a {{
    
  }}
  .es-button {{
    mso-style-priority:100!important;
    text-decoration:none!important;
  }}
  a[x-apple-data-detectors] {{
    color:inherit!important;
    text-decoration:none!important;
    font-size:inherit!important;
    font-family:inherit!important;
    font-weight:inherit!important;
    line-height:inherit!important;
  }}
  .es-desk-hidden {{
    display:none;
    float:left;
    overflow:hidden;
    width:0;
    max-height:0;
    line-height:0;
    mso-hide:all;
  }}
  @media only screen and (max-width:600px) {{p, ul li, ol li {{ margin-bottom:11px!important }} .es-header-body p, .es-header-body ul li, .es-header-body ol li {{ margin-bottom:9px!important }} .es-footer-body p, .es-footer-body ul li, .es-footer-body ol li {{ margin-bottom:8px!important }} .es-infoblock p, .es-infoblock ul li, .es-infoblock ol li {{ margin-bottom:8px!important }} p, ul li, ol li, a {{ line-height:120%!important }} h1, h2, h3, h1 a, h2 a, h3 a {{ line-height:100%!important }} h1 {{ font-size:30px!important; text-align:center; margin-bottom:15px }} h2 {{ font-size:24px!important; text-align:center; margin-bottom:12px }} h3 {{ font-size:20px!important; text-align:center; margin-bottom:10px }} .es-header-body h1 a, .es-content-body h1 a, .es-footer-body h1 a {{ font-size:30px!important; text-align:center }} .es-header-body h2 a, .es-content-body h2 a, .es-footer-body h2 a {{ font-size:24px!important; text-align:center }} .es-header-body h3 a, .es-content-body h3 a, .es-footer-body h3 a {{ font-size:20px!important; text-align:center }} .es-menu td a {{ font-size:12px!important }} .es-header-body p, .es-header-body ul li, .es-header-body ol li, .es-header-body a {{ font-size:14px!important }} .es-content-body p, .es-content-body ul li, .es-content-body ol li, .es-content-body a {{ font-size:14px!important }} .es-footer-body p, .es-footer-body ul li, .es-footer-body ol li, .es-footer-body a {{ font-size:12px!important }} .es-infoblock p, .es-infoblock ul li, .es-infoblock ol li, .es-infoblock a {{ font-size:12px!important }} *[class=\"gmail-fix\"] {{ display:none!important }} .es-m-txt-c, .es-m-txt-c h1, .es-m-txt-c h2, .es-m-txt-c h3 {{ text-align:center!important }} .es-m-txt-r, .es-m-txt-r h1, .es-m-txt-r h2, .es-m-txt-r h3 {{ text-align:right!important }} .es-m-txt-l, .es-m-txt-l h1, .es-m-txt-l h2, .es-m-txt-l h3 {{ text-align:left!important }} .es-m-txt-r img, .es-m-txt-c img, .es-m-txt-l img {{ display:inline!important }} .es-button-border {{ display:inline-block!important }} a.es-button, button.es-button {{ font-size:18px!important; display:inline-block!important }} .es-adaptive table, .es-left, .es-right {{ width:100%!important }} .es-content table, .es-header table, .es-footer table, .es-content, .es-footer, .es-header {{ width:100%!important; max-width:600px!important }} .es-adapt-td {{ display:block!important; width:100%!important }} .adapt-img {{ width:100%!important; height:auto!important }} .es-m-p0 {{ padding:0!important }} .es-m-p0r {{ padding-right:0!important }} .es-m-p0l {{ padding-left:0!important }} .es-m-p0t {{ padding-top:0!important }} .es-m-p0b {{ padding-bottom:0!important }} .es-m-p20b {{ padding-bottom:20px!important }} .es-mobile-hidden, .es-hidden {{ display:none!important }} tr.es-desk-hidden, td.es-desk-hidden, table.es-desk-hidden {{ width:auto!important; overflow:visible!important; float:none!important; max-height:inherit!important; line-height:inherit!important }} tr.es-desk-hidden {{ display:table-row!important }} table.es-desk-hidden {{ display:table!important }} td.es-desk-menu-hidden {{ display:table-cell!important }} .es-menu td {{ width:1%!important }} table.es-table-not-adapt, .esd-block-html table {{ width:auto!important }} table.es-social {{ display:inline-block!important }} table.es-social td {{ display:inline-block!important }} .es-desk-hidden {{ display:table-row!important; width:auto!important; overflow:visible!important; max-height:inherit!important }} .es-m-p5 {{ padding:5px!important }} .es-m-p5t {{ padding-top:5px!important }} .es-m-p5b {{ padding-bottom:5px!important }} .es-m-p5r {{ padding-right:5px!important }} .es-m-p5l {{ padding-left:5px!important }} .es-m-p10 {{ padding:10px!important }} .es-m-p10t {{ padding-top:10px!important }} .es-m-p10b {{ padding-bottom:10px!important }} .es-m-p10r {{ padding-right:10px!important }} .es-m-p10l {{ padding-left:10px!important }} .es-m-p15 {{ padding:15px!important }} .es-m-p15t {{ padding-top:15px!important }} .es-m-p15b {{ padding-bottom:15px!important }} .es-m-p15r {{ padding-right:15px!important }} .es-m-p15l {{ padding-left:15px!important }} .es-m-p20 {{ padding:20px!important }} .es-m-p20t {{ padding-top:20px!important }} .es-m-p20r {{ padding-right:20px!important }} .es-m-p20l {{ padding-left:20px!important }} .es-m-p25 {{ padding:25px!important }} .es-m-p25t {{ padding-top:25px!important }} .es-m-p25b {{ padding-bottom:25px!important }} .es-m-p25r {{ padding-right:25px!important }} .es-m-p25l {{ padding-left:25px!important }} .es-m-p30 {{ padding:30px!important }} .es-m-p30t {{ padding-top:30px!important }} .es-m-p30b {{ padding-bottom:30px!important }} .es-m-p30r {{ padding-right:30px!important }} .es-m-p30l {{ padding-left:30px!important }} .es-m-p35 {{ padding:35px!important }} .es-m-p35t {{ padding-top:35px!important }} .es-m-p35b {{ padding-bottom:35px!important }} .es-m-p35r {{ padding-right:35px!important }} .es-m-p35l {{ padding-left:35px!important }} .es-m-p40 {{ padding:40px!important }} .es-m-p40t {{ padding-top:40px!important }} .es-m-p40b {{ padding-bottom:40px!important }} .es-m-p40r {{ padding-right:40px!important }} .es-m-p40l {{ padding-left:40px!important }} }}
  @media screen and (max-width:384px) {{.mail-message-content {{ width:414px!important }} }}
  </style>
   </head>
   <body style=\"width:100%;font-family:'open sans', 'helvetica neue', helvetica, arial, sans-serif;-webkit-text-size-adjust:100%;-ms-text-size-adjust:100%;Margin:0\">
    <div dir=\"ltr\" class=\"es-wrapper-color\" lang=\"en\" style=\"background-color:#131313\">
     <table class=\"es-wrapper\" width=\"100%\" cellspacing=\"0\" cellpadding=\"0\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px;width:100%;height:100%;background-repeat:repeat;background-position:center top;background-color:#131313\">
       <tr>
        <td valign=\"top\" style=\"Margin:0\">
         <table cellpadding=\"0\" cellspacing=\"0\" class=\"es-header\" align=\"center\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px;table-layout:fixed !important;width:100%;background-color:transparent;background-repeat:repeat;background-position:center top\">
           <tr>
            <td align=\"center\" style=\"Margin:0\">
             <table bgcolor=\"#ffffff\" class=\"es-header-body\" align=\"center\" cellpadding=\"0\" cellspacing=\"0\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px;background-color:transparent;width:700px\">
               <tr>
                <td align=\"left\" style=\"padding:20px;Margin:0\">
                 <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px\">
                   <tr>
                    <td class=\"es-m-p0r\" valign=\"top\" align=\"center\" style=\"width:660px\">
                     <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px\">
                       <tr>
                        <td align=\"center\" class=\"es-m-txt-c\" style=\"font-size:0px\"><a target=\"_blank\" href=\"https://viewstripo.email\" style=\"-webkit-text-size-adjust:none;-ms-text-size-adjust:none;mso-line-height-rule:exactly;text-decoration:underline;color:#FFFFFF;font-size:12px\"><img src=\"https://ecwnuzs.stripocdn.email/content/guids/89070ba8-83e1-4d67-b646-4663532227d2/images/masterlogov2_1.png\" alt=\"Logo\" style=\"display:block;border:0;outline:none;text-decoration:none;-ms-interpolation-mode:bicubic\" title=\"Logo\" height=\"70\"></a></td>
                       </tr>
                     </table></td>
                   </tr>
                 </table></td>
               </tr>
             </table></td>
           </tr>
         </table>
         <table cellpadding=\"0\" cellspacing=\"0\" class=\"es-content\" align=\"center\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px;table-layout:fixed !important;width:100%\">
           <tr>
            <td align=\"center\" style=\"Margin:0\">
             <table bgcolor=\"#ffffff\" class=\"es-content-body\" align=\"center\" cellpadding=\"0\" cellspacing=\"0\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px;background-color:#FFFFFF;border-radius:50px 50px 0 0;width:700px\">
               <tr>
                <td align=\"left\" style=\"Margin:0\">
                 <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px\">
                   <tr>
                    <td align=\"center\" valign=\"top\" style=\"width:700px\">
                     <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:separate;border-spacing:0px;border-radius:4px\">
                       <tr>
                        <td align=\"center\" class=\"es-m-p20r es-m-p20l\" style=\"padding-top:5px\"><h2 style=\"line-height:48px;mso-line-height-rule:exactly;font-family:'trebuchet ms', 'lucida grande', 'lucida sans unicode', 'lucida sans', tahoma, sans-serif;font-size:24px;font-style:normal;font-weight:bold;color:#000000;margin-bottom:12px\"><b>{customer_name} - {so_num}</b></h2></td>
                       </tr>
                     </table></td>
                   </tr>
                 </table></td>
               </tr>
               <tr>
                <td align=\"left\" bgcolor=\"#22272f\" style=\"padding-left:20px;padding-right:20px;padding-top:40px;background-color:#22272f\">
                 <table cellpadding=\"0\" cellspacing=\"0\" class=\"es-left\" align=\"left\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px;float:left\">
                   <tr>
                    <td class=\"es-m-p20b\" align=\"left\" style=\"width:315px\">
                     <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px\">
                       <tr>
                        <td align=\"center\" style=\"Margin:0\"><h2 style=\"line-height:24px;mso-line-height-rule:exactly;font-family:'times new roman', times, baskerville, georgia, serif;font-size:24px;font-style:normal;font-weight:bold;color:#fcfdfd;margin-bottom:12px;text-align:center\"><strong>Customer Info</strong></h2>
                         <table border=\"2\" align=\"center\" cellspacing=\"2\" cellpadding=\"2\" class=\"es-table\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px;width:100%\">
                           <tr>
                            <td style=\"color:#ffffff\"><strong>Phone #</strong></td>
                            <td style=\"text-align:center;color:#ffffff\">{phone1}</td>
                           </tr>
                           <tr>
                            <td style=\"color:#ffffff\"><strong>Phone # 2</strong></td>
                            <td style=\"text-align:center;color:#ffffff\">{phone2}</td>
                           </tr>
                           <tr>
                            <td style=\"color:#ffffff\"><strong>Email</strong></td>
                            <td style=\"color:#ffffff;text-align:center\">{cust_email}</td>
                           </tr>
                           <tr>
                            <td style=\"color:#ffffff\"><strong>Ticket Total</strong></td>
                            <td style=\" text-align:center\">${inv_amt}</td>
                           </tr>
                           <tr>
                            <td style=\"color:#ffffff\"><strong>Last SI #</strong></td>
                            <td style=\" text-align:center\">{last_inv_num}</td>
                           </tr>
                           <tr>
                            <td style=\"color:#ffffff\"><strong>Last SI $</strong></td>
                            <td style=\" text-align:center\">${last_inv_amt}</td>
                           </tr>
                         </table></td>
                       </tr>
                     </table></td>
                   </tr>
                 </table>
                 <table cellpadding=\"0\" cellspacing=\"0\" class=\"es-right\" align=\"right\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px;float:right\">
                   <tr>
                    <td align=\"left\" style=\"width:315px\">
                     <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px\">
                       <tr>
                        <td align=\"left\" style=\"Margin:0\"><h2 style=\"line-height:24px;mso-line-height-rule:exactly;font-family:'times new roman', times, baskerville, georgia, serif;font-size:24px;font-style:normal;font-weight:bold;color:#ffffff;margin-bottom:12px;text-align:center\"><strong>Hardware Info</strong></h2>
                         <table border=\"2\" align=\"center\" cellspacing=\"2\" cellpadding=\"2\" class=\"es-table\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px;width:100%\">
                          {specs}
                         </table><p style=\"-webkit-text-size-adjust:none;-ms-text-size-adjust:none;mso-line-height-rule:exactly;font-family:'open sans', 'helvetica neue', helvetica, arial, sans-serif;line-height:22px;margin-bottom:11px;color:#081D36;font-size:18px\"><br></p></td>
                       </tr>
                     </table></td>
                   </tr>
                   <tr>
                    <td align=\"left\" style=\"width:315px\">
                     <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px\">
                       <tr>
                        <td align=\"left\" style=\"Margin:0\"><h3 style=\"line-height:20px;mso-line-height-rule:exactly;font-family:'times new roman', times, baskerville, georgia, serif;font-size:20px;font-style:normal;font-weight:bold;color:#ffffff;margin-bottom:10px;text-align:center\"><strong>Drives</strong></h3>
                         <table border=\"2\" align=\"center\" cellspacing=\"2\" cellpadding=\"2\" class=\"es-table\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px;width:100%\">
                            {final_disk}
                         </table><p style=\"-webkit-text-size-adjust:none;-ms-text-size-adjust:none;mso-line-height-rule:exactly;font-family:'open sans', 'helvetica neue', helvetica, arial, sans-serif;line-height:22px;margin-bottom:11px;color:#081D36;font-size:18px\"><br></p></td>
                       </tr>
                     </table></td>
                   </tr>
                 </table></td>
               </tr>
             </table></td>
           </tr>
         </table>
         <table cellpadding=\"0\" cellspacing=\"0\" class=\"es-content\" align=\"center\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px;table-layout:fixed !important;width:100%\">
           <tr>
            <td align=\"center\" style=\"Margin:0\">
             <table bgcolor=\"#ffffff\" class=\"es-content-body\" align=\"center\" cellpadding=\"0\" cellspacing=\"0\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px;background-color:#FFFFFF;width:700px\">
               <tr>
                <td align=\"left\" bgcolor=\"#2d3644\" style=\"background-color:#2d3644\">
                 <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px\">
                   <tr>
                    <td align=\"left\" style=\"width:700px\">
                     <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px\">
                       <tr>
                        <td align=\"center\" style=\"padding:10px;font-size:0\">
                         <table border=\"0\" width=\"100%\" height=\"100%\" cellpadding=\"0\" cellspacing=\"0\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px\">
                           <tr>
                            <td style=\"border-bottom:1px solid #11feda;background:unset;height:1px;width:100%;margin:0px\"></td>
                           </tr>
                         </table></td>
                       </tr>
                       <tr>
                        <td align=\"center\" style=\"Margin:0\"><h2 style=\"line-height:36px;mso-line-height-rule:exactly;font-family:'trebuchet ms', 'lucida grande', 'lucida sans unicode', 'lucida sans', tahoma, sans-serif;font-size:24px;font-style:normal;font-weight:bold;color:#ffffff;margin-bottom:12px\">Service Notes</h2></td>
                       </tr>
                       <tr>
                        <td align=\"center\" style=\"padding:10px;font-size:0\">
                         <table border=\"0\" width=\"100%\" height=\"100%\" cellpadding=\"0\" cellspacing=\"0\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px\">
                           <tr>
                            <td style=\"border-bottom:1px solid #11feda;background:unset;height:1px;width:100%;margin:0px\"></td>
                           </tr>
                         </table></td>
                       </tr>
                     </table></td>
                   </tr>
                 </table></td>
               </tr>
               <tr>
                <td align=\"left\" bgcolor=\"#22272f\" style=\"padding-left:20px;padding-right:20px;padding-bottom:25px;padding-top:40px;background-color:#22272f\">
                 <table cellpadding=\"0\" cellspacing=\"0\" class=\"es-left\" align=\"left\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px;float:left\">
                   <tr>
                    <td class=\"es-m-p20b\" align=\"left\" style=\"width:320px\">
                     <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px\">
                       <tr>
                        <td align=\"center\" style=\"Margin:0\"><h2 style=\"line-height:36px;mso-line-height-rule:exactly;font-family:'trebuchet ms', 'lucida grande', 'lucida sans unicode', 'lucida sans', tahoma, sans-serif;font-size:24px;font-style:normal;font-weight:bold;color:#ffffff;margin-bottom:12px\">Checkin Notes</h2></td>
                       </tr>
                       <tr>
                        <td align=\"center\" style=\"Margin:0\"><p style=\"-webkit-text-size-adjust:none;-ms-text-size-adjust:none;mso-line-height-rule:exactly;font-family:'open sans', 'helvetica neue', helvetica, arial, sans-serif;line-height:27px;margin-bottom:11px;color:#ffffff;font-size:18px\">{checkin_notes}</p></td>
                       </tr>
                     </table></td>
                   </tr>
                 </table><!--[if mso]></td><td style=\"width:20px\"></td><td style=\"width:319px\" valign=\"top\"><![endif]-->
                 <table cellpadding=\"0\" cellspacing=\"0\" class=\"es-right\" align=\"right\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px;float:right\">
                   <tr>
                    <td align=\"left\" style=\"width:319px\">
                     <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px\">
                       <tr>
                        <td align=\"center\" style=\"Margin:0\"><h2 style=\"line-height:36px;mso-line-height-rule:exactly;font-family:'trebuchet ms', 'lucida grande', 'lucida sans unicode', 'lucida sans', tahoma, sans-serif;font-size:24px;font-style:normal;font-weight:bold;color:#ffffff;margin-bottom:12px\">Recommendations</h2></td>
                       </tr>
                       <tr>
                        <td align=\"center\" style=\"Margin:0\"><p style=\"-webkit-text-size-adjust:none;-ms-text-size-adjust:none;mso-line-height-rule:exactly;font-family:'open sans', 'helvetica neue', helvetica, arial, sans-serif;line-height:27px;margin-bottom:11px;color:#ffffff;font-size:18px\">{recommendations}</p></td>
                       </tr>
                     </table></td>
                   </tr>
                 </table></td>
               </tr>
             </table></td>
           </tr>
         </table>
         <table cellpadding=\"0\" cellspacing=\"0\" class=\"es-footer\" align=\"center\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px;table-layout:fixed !important;width:100%;background-color:transparent;background-repeat:repeat;background-position:center top\">
           <tr>
            <td align=\"center\" style=\"Margin:0\">
             <table bgcolor=\"#ffffff\" class=\"es-footer-body\" align=\"center\" cellpadding=\"0\" cellspacing=\"0\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px;background-color:#FFFFFF;border-radius:0 0 50px 50px;width:700px\">
               <tr>
                <td align=\"left\" style=\"padding:5px;Margin:0\">
                 <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px\">
                   <tr>
                    <td align=\"center\" valign=\"top\" style=\"width:690px\">
                     <table cellpadding=\"0\" cellspacing=\"0\" width=\"100%\" style=\"mso-table-lspace:0pt;mso-table-rspace:0pt;border-collapse:collapse;border-spacing:0px\">
                       <tr>
                        <td align=\"center\" class=\"made_with\" style=\"font-size:0px\"><a target=\"_blank\" href=\"https://viewstripo.email/?utm_source=templates&utm_medium=email&utm_campaign=gadget_11&utm_content=santa_claus_brought_gifts\" style=\"-webkit-text-size-adjust:none;-ms-text-size-adjust:none;mso-line-height-rule:exactly;text-decoration:underline;color:#081D36;font-size:14px\"><img src=\"https://ecwnuzs.stripocdn.email/content/guids/89070ba8-83e1-4d67-b646-4663532227d2/images/pcllogo.png\" alt width=\"60\" style=\"display:block;border:0;outline:none;text-decoration:none;-ms-interpolation-mode:bicubic\"></a></td>
                       </tr>
                     </table></td>
                   </tr>
                 </table></td>
               </tr>
             </table></td>
           </tr>
         </table></td>
       </tr>
     </table>
    </div>
   </body>
  </html>
  ").to_string();

  html_string
}