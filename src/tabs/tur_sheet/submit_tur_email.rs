use std::{collections::HashMap, path::PathBuf};
use lettre::{{Message, SmtpTransport, Transport}, transport::smtp::authentication::Credentials, message::header::ContentType};
use serde_json::Value;

use crate::{app_state::MastertechContext, database::schema::Store};

use super::email_builder::{email_builder, Info};

impl MastertechContext{
    pub fn submit_tur_email(&mut self) {
        let cust = &self.ticket_info.customer_name;
        let so_num = &self.so_number;

        let mut salesman_map = HashMap::new();
        let mut tech_map = HashMap::new();

        let salesman = &self.salesman; // &format!("{:?}", &self.salesman_cbox);
        let checkin_rep = &self.ticket_info.checkin_rep;
        let technician = &self.technician; // &format!("{:?}", &self.techs_cbox);

        salesman_map.insert("Jake", "1202792432658520");
        salesman_map.insert("Danny", "1202791016369879");
        tech_map.insert("Logan", "1199992640930465");
        tech_map.insert("Bread", "1202792432421640");
        tech_map.insert("Taco", "1202792432551073");

        // let assigned_salesman = salesman_map.get(salesman.as_str()).unwrap_or(&"1202792432658520").to_string();
        // let assigned_tech = tech_map.get(technician.as_str()).unwrap_or(&"1199992640930465").to_string();

        let hdd_test = &format!("{:?}", &self.hdd_test_cbox);
        let ram_test = &format!("{:?}", &self.ram_test_cbox);
        let ssd_test = &format!("{:?}", &self.ssd_test_cbox);

        let checkin_notes = &self.ticket_info.checkin_notes;
        let recommendations = &self.recommendations;   

        let mut _attached_file: Option<PathBuf> = None;
        if let Some(file) = &self.opened_file{
            _attached_file = Some(file.to_path_buf());
        }

        let mut _specs = String::new();
        let cps = self.current_antivirus.clone();
        let seb_info = self.seb_info.clone().unwrap_or_default();

        

        let mut final_disk = String::new();
        let mut each_disk = String::new();
                                                
        let cust_code = &self.ticket_info.cust_code;
        let doc_alias = &self.ticket_info.doc_alias;
        let _department = &self.ticket_info.dep;
        //let juris = &self.ticket_info.juris;
        let ticket_total = &self.ticket_info.ticket_total;
        let cust_email = &self.ticket_info.customer_email;
        let last_inv_num = &self.ticket_info.last_invoice_number;
        let last_inv_amt = &self.ticket_info.last_invoice_amount;
        let total_inv_num = &self.ticket_info.total_invoice_count;
        let phone1 = &self.ticket_info.customer_phone_1;
        let phone2 = &self.ticket_info.customer_phone_2;
        let mut phone_2 = String::new();
        if !phone2.is_empty(){
            phone_2 = format!("<tr>
            <td style=\"padding:1px 1px\">Phone #2</td>
            <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{phone2}</td>
            </tr>");
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
            <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">${ticket_total}</td>
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
        if self.send_specs == true{
            self.output_text.clear();
            self.output_text += "pulling system information. Please wait a moment..\n";
            let system_name = &self.system_info.hostname;
            let os = &self.system_info.operating_system;
            let cpu_name = &self.system_info.cpu;
            let total_ram = &self.system_info.ram;
            let gpu = &self.system_info.gpu.clone();

            for index in 0..self.disk_num
            {
                if let Some(disk) = self.disks.get(index)
                {
                    let drive_letter = format!("{}", disk.get("drive_letter").and_then(Value::as_str).unwrap_or(""));
                    let drive_type = disk.get("drive_type").and_then(Value::as_str).unwrap_or("");
                    let space_left = format!("{} Gb", disk.get("space_left").and_then(Value::as_str).unwrap_or(""));
                    let total_size = format!("{} Gb", disk.get("total_size").and_then(Value::as_str).unwrap_or(""));

                    each_disk += &format!("
                    <tr>
                    <td style=\"padding:1px 1px\">        {drive_letter}</td>
                    <td style=\"padding:1px 1px\">        {drive_type}</td>
                    <td style=\"padding:1px 1px\">        {space_left}</td>
                    <td style=\"padding:1px 1px\">        {total_size}</td>
                    </tr>
                    ");

                    final_disk = format!
                        ("
                        <tr>
                            <td style=\"padding:1px 4px\">Letter</td>
                            <td style=\"padding:1px 4px\">Drive Type</td>
                            <td style=\"padding:1px 4px\">Avail Space</td>
                            <td style=\"padding:1px 4px\">Total Space</td>
                        </tr>
                        {each_disk}
                    ");

                }
            }

            _specs = format!("
            <table>
                <tr>
                    <td style=\"text-align:center;\" colspan=\"3\" data-cell-widths=\"130,200,200\" width=\"450\"
                    >              <code>       Computer Info        </code></td>
                </tr>
                <tr>
                    <td>PC Name</td>
                    <td colspan=\"2\" data-cell-widths=\"150,150\">{system_name}</td>
                </tr>
                <tr>
                    <td>OS</td>
                    <td colspan=\"2\" data-cell-widths=\"150,150\">{os}</td>
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
                    <td colspan=\"2\" data-cell-widths=\"150,150\">{seb_info:#?}</td>
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
            _specs = "Computer information was not sent with ticket".to_string();
        }
        
        let store: &Store = &self.ticket_info.dep;

        let _mtech_username = dotenv::var("MTECH_EMAIL").unwrap_or("not provided".to_string());
        let _mtech_password = dotenv::var("MTECH_PASS").unwrap_or("not provided".to_string());
        let store_email = store.store_email();

        let system_name = &self.system_info.hostname;
        let cpu_name = &self.system_info.cpu;
        let total_ram = &self.system_info.ram;
        let gpu = &self.system_info.gpu.clone();
        let mut final_disk = String::new();
        let mut each_disk = String::new();

        for index in 0..self.disk_num
        {
            if let Some(disk) = self.disks.get(index)
            {
                let disk_letter = format!("{}", disk.get("letter").and_then(Value::as_str).unwrap_or(""));
                let drive_type = disk.get("drive_type").and_then(Value::as_str).unwrap_or("");
                let disk_available = format!("{} Gb", disk.get("space_left").and_then(Value::as_str).unwrap_or(""));
                let disk_total = format!("{} Gb", disk.get("total_size").and_then(Value::as_str).unwrap_or(""));

                each_disk += &format!("
                <tr>
                    <td style=\"text-align: center; padding:1px 1px color: #ffffff\">{disk_letter}</td>
                    <td style=\"text-align: center; padding:1px 1px color: #ffffff\">{drive_type}</td>
                    <td style=\"text-align: center; padding:1px 1px color: #ffffff\">{disk_available}</td>
                    <td style=\"text-align: center; padding:1px 1px color: #ffffff\">{disk_total}</td>
                </tr>
                ");

                final_disk = format!
                    ("
                    <tr>
                        <td style=\"padding:1px 4px; text-align: center; \">Letter</td>
                        <td style=\"padding:1px 4px; text-align: center; \">Type</td>
                        <td style=\"padding:1px 4px; text-align: center; \">Avail Space</td>
                        <td style=\"padding:1px 4px; text-align: center; \">Total Space</td>
                    </tr>
                    {each_disk}
                ");

            }
        }

        _specs = format!("
        <tr>
            <td style=\"color: #ffffff;\"><strong>CPU</strong></td>
            <td style=\"text-align: center; color: #ffffff;\">{cpu_name}</td>
        </tr>
        <tr>
            <td style=\"color: #ffffff;\"><strong>GPU</strong></td>
            <td style=\"text-align: center; color: #ffffff;\">{gpu}</td>
        </tr>
        <tr>
            <td style=\"color: #ffffff;\"><strong>RAM</strong></td>
            <td style=\"text-align: center; color: #ffffff;\">{total_ram} Gb</td>
        </tr>
        <tr>
            <td style=\"color: #ffffff;\"><b>System Name</b></td>
            <td>
                <p style=\"text-align: center; color: #ffffff;\">{system_name}</p>
            </td>
        </tr>
        <tr>
            <td style=\"color: #ffffff;\"><b>CPS</b></td>
            <td>
                <p style=\"text-align: center; color: #ffffff;\">{cps}</p>
            </td>
        </tr>
        ");



        let info = Info{
            customer_name: cust.to_string(),
            so_num: so_num.to_string(),
            hdd_test: hdd_test.to_string(),
            ram_test: ram_test.to_string(),
            ssd_test: ssd_test.to_string(),
            checkin_notes: checkin_notes.to_string(),
            recommendations: recommendations.to_string(),
            specs: _specs,
            cps,
            cust_code: cust_code.to_string(),
            doc_alias: doc_alias.to_string(),
            inv_amt: ticket_total.to_string(),
            cust_email: cust_email.to_string(),
            last_inv_num: last_inv_num.to_string(),
            last_inv_amt: last_inv_amt.to_string(),
            total_inv_num: total_inv_num.to_string(),
            phone1: phone1.to_string(),
            phone2: phone2.to_string(),

            final_disk,

            salesman: salesman.to_string(),
            checkin_rep: checkin_rep.to_string(),
            technician: technician.to_string(),
            extra_customer_info,
        };

        let html = email_builder(info);
        
        let email = Message::builder()
            .from("TUR SHEET <pcl.mastertech@gmail.com>".parse().unwrap())
            .to(store_email.parse().unwrap())
            .subject(format!("{cust} - {so_num}"))
            .header(ContentType::TEXT_HTML)
            .body(html)
            .unwrap();

        let creds = Credentials::new("pcl.mastertech@gmail.com".to_owned(), "pgumcgekyrcqadah".to_owned());

        // Open a remote connection to gmail
        let mailer = SmtpTransport::relay("smtp.gmail.com")
            .unwrap()
            .credentials(creds)
            .build();

        self.output_text += format!("\n {store_email} {cust_email}").as_str();

        // Send the email
        match mailer.send(&email) {
            Ok(_) => println!("Email sent successfully!"),
            Err(e) => {
                self.output_text += format!("\n{e:?}").as_str();
                //println!("Could not send email: {e:?}")
            },
        }
    }
}