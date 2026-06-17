# FINAL TARGET & RULE — FoFreeXit

> File này là LUẬT TỐI THƯỢỢNG của dự án. Mọi task đã, đang và sẽ làm đều PHẢI tuân thủ.

## MỤC TIÊU CUỐI CÙNG
**Mục tiêu là tạo ra phần mềm có cách dùng, hiệu năng, chức năng, giao diện và trải nghiệm y hệt Foxit.**

## LUẬT BẤT BIẾN (NO-COMPROMISE RULE)
**Nếu tính năng Foxit có mà thư viện không đáp ứng được thì hãy update code cho thư viện nếu có thể, nếu không thì tìm thư viện khác hoặc tự code để xử lý vấn đề. Không thỏa hiệp. Mọi tính năng đã, đang và sẽ làm phải có trải nghiệm và chức năng tương đương Foxit, không thỏa hiệp cắt giảm tính năng vì bất kì lí do gì. Làm được bằng bất kì cách nào.**

## DIỄN GIẢI ÁP DỤNG (bắt buộc với mọi task)
1. Trước khi làm bất kỳ tính năng nào: **tìm hiểu Foxit làm tính năng đó thế nào** (UI, thao tác, tuỳ chọn, định dạng, cách sửa/xoá, màu sắc...) và lấy đó làm chuẩn nghiệm thu.
2. **Không được cắt giảm** vì "thư viện không hỗ trợ". Thứ tự xử lý khi vướng thư viện:
   a. Sửa/patch/fork thư viện (vendor as path dependency) để thêm khả năng còn thiếu.
   b. Nếu không được → tìm thư viện khác phù hợp hơn.
   c. Nếu không được → tự viết (thao tác tầng PDF object, appearance stream, v.v.).
3. Mỗi tính năng phải đạt **cả 5 mặt**: cách dùng, hiệu năng, chức năng, giao diện, trải nghiệm — tương đương Foxit.
4. Mỗi tính năng phải có **test/nghiệm thu** chứng minh đạt chuẩn Foxit trước khi coi là xong; nêu rõ phần nào chưa đạt (nếu tạm thời) và kế hoạch đạt 100%.
5. Không dùng dialog trình duyệt (prompt/alert) thay cho UI chuyên nghiệp.

## CHECKLIST GẮN KÈM MỌI TASK
- [ ] Đã nghiên cứu Foxit làm tính năng này thế nào?
- [ ] UI/thao tác/tuỳ chọn có tương đương Foxit?
- [ ] Có vướng thư viện không? Nếu có, đã patch/đổi/tự-code (KHÔNG cắt giảm)?
- [ ] Có test/nghiệm thu so với chuẩn Foxit?
- [ ] Hiệu năng & trải nghiệm mượt như Foxit?
